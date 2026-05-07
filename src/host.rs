use crate::config::{
    BuiltInProviderConfig, ChatGptAuthConfig, CliConfigError, EnvHeaderConfig, HostProfileConfig,
    ProfileCompactionConfig, RegistryStoreConfig, RuntimeProfileConfig, resolve_chatgpt_token_file,
};
use noloong_agent::{
    AgentManifest, AgentSession, ManifestPatch,
    interaction::{
        AgentRuntimeProfile, AgentSessionRegistry, AgentSessionRegistryStore,
        InMemoryAgentSessionRegistryStore, InteractionError, InteractionFuture,
        InteractionProfileDescriptor, OpenDalAgentSessionRegistryStore,
        OpenDalAgentSessionRegistryStoreConfig, SqlAgentSessionRegistryStore,
        SqlAgentSessionRegistryStoreConfig,
    },
};
use noloong_agent_core::{
    AgentCoreError, AgentRuntime, AnthropicMessagesProvider, AnthropicMessagesProviderConfig,
    BoxFuture, CancellationToken, ChatCompletionsProvider, ChatCompletionsProviderConfig,
    ContextCompactionConfig, ContextCompactionMode, ContextCompactor, HttpAuthContext,
    HttpAuthHeader, HttpAuthHeaders, HttpAuthProvider, ModelProvider, ResponsesApiProvider,
    ResponsesApiProviderConfig,
};
use noloong_openai::{
    auth::{ChatGptAuthManager, ChatGptTokenStorage, ChatGptTokenStore},
    compact::{OpenAiResponsesCompactor, OpenAiResponsesCompactorConfig},
};
use opendal::{
    Operator,
    services::{Fs, Memory},
};
use serde_json::json;
use std::{env, sync::Arc, time::Duration};
use thiserror::Error;

const DEFAULT_CHATGPT_COMPACTION_TRIGGER_TOKENS: u64 = 128_000;
const DEFAULT_CHATGPT_COMPACTION_RESERVE_TOKENS: u64 = 16_384;
const DEFAULT_CHATGPT_COMPACTION_KEEP_RECENT_TOKENS: u64 = 20_000;

pub async fn build_registry(
    config: &HostProfileConfig,
) -> Result<AgentSessionRegistry, HostBuildError> {
    config.validate()?;
    let profiles = config
        .profiles
        .iter()
        .map(RuntimeProfile::try_from_config)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|profile| Arc::new(profile) as Arc<dyn AgentRuntimeProfile>)
        .collect::<Vec<_>>();
    let default_profile_id = config
        .default_profile_id
        .clone()
        .unwrap_or_else(|| config.profiles[0].profile_id.clone());
    let store = build_registry_store(&config.registry_store).await?;
    AgentSessionRegistry::with_store(default_profile_id, profiles, store)
        .map_err(HostBuildError::Interaction)
}

async fn build_registry_store(
    config: &RegistryStoreConfig,
) -> Result<Arc<dyn AgentSessionRegistryStore>, HostBuildError> {
    match config {
        RegistryStoreConfig::Memory => Ok(Arc::new(InMemoryAgentSessionRegistryStore::default())),
        RegistryStoreConfig::Sqlite { database_url }
        | RegistryStoreConfig::Postgres { database_url } => {
            let store = SqlAgentSessionRegistryStore::connect(
                SqlAgentSessionRegistryStoreConfig::new(database_url),
            )
            .await?;
            Ok(Arc::new(store))
        }
        RegistryStoreConfig::ObjectMemory { prefix } => {
            Ok(Arc::new(OpenDalAgentSessionRegistryStore::new(
                Operator::new(Memory::default())
                    .map_err(opendal_error)?
                    .finish(),
                OpenDalAgentSessionRegistryStoreConfig::new(prefix),
            )))
        }
        RegistryStoreConfig::ObjectFs { root, prefix } => {
            let builder = Fs::default().root(root);
            Ok(Arc::new(OpenDalAgentSessionRegistryStore::new(
                Operator::new(builder).map_err(opendal_error)?.finish(),
                OpenDalAgentSessionRegistryStoreConfig::new(prefix),
            )))
        }
    }
}

#[derive(Clone)]
struct RuntimeProfile {
    descriptor: InteractionProfileDescriptor,
    provider: Arc<dyn ModelProvider>,
    compaction: Option<RuntimeCompaction>,
}

impl RuntimeProfile {
    fn try_from_config(config: &RuntimeProfileConfig) -> Result<Self, HostBuildError> {
        let mut validated_manifest = AgentManifest::default();
        let mut default_manifest_patches = config
            .plugins
            .iter()
            .cloned()
            .map(|plugin| ManifestPatch::RegisterPlugin { plugin })
            .collect::<Vec<_>>();
        default_manifest_patches.extend(config.manifest_patches.iter().cloned());
        for patch in &default_manifest_patches {
            validated_manifest
                .apply_patch(patch.clone())
                .map_err(|error| {
                    HostBuildError::Config(CliConfigError::ParseConfig(error.to_string()))
                })?;
        }
        let provider = build_provider(&config.profile_id, &config.provider)?;
        let compaction = build_profile_compaction(
            &config.profile_id,
            &config.compaction,
            provider.chatgpt_compact.as_ref(),
        )?;
        Ok(Self {
            descriptor: InteractionProfileDescriptor {
                profile_id: config.profile_id.clone(),
                display_name: config.display_name.clone(),
                description: config.description.clone(),
                default_manifest_patches,
                metadata: config.metadata.clone(),
            },
            provider: provider.provider,
            compaction,
        })
    }
}

impl AgentRuntimeProfile for RuntimeProfile {
    fn descriptor(&self) -> InteractionProfileDescriptor {
        self.descriptor.clone()
    }

    fn build_runtime<'a>(
        &'a self,
        session: &'a AgentSession,
        _manifest: &'a AgentManifest,
    ) -> InteractionFuture<'a, AgentRuntime> {
        Box::pin(async move {
            let mut builder = session
                .runtime_builder()
                .with_model_provider(Arc::clone(&self.provider));
            if let Some(compaction) = &self.compaction {
                builder = builder.with_context_compactor(
                    compaction.config.clone(),
                    Arc::clone(&compaction.compactor),
                );
            }
            builder = builder
                .with_manifest_plugins()
                .await
                .map_err(InteractionError::from)?;
            builder.build().map_err(InteractionError::from)
        })
    }
}

#[derive(Clone)]
struct RuntimeCompaction {
    config: ContextCompactionConfig,
    compactor: Arc<dyn ContextCompactor>,
}

struct BuiltProvider {
    provider: Arc<dyn ModelProvider>,
    chatgpt_compact: Option<ChatGptCompactSource>,
}

impl BuiltProvider {
    fn model(provider: Arc<dyn ModelProvider>) -> Self {
        Self {
            provider,
            chatgpt_compact: None,
        }
    }
}

#[derive(Clone)]
struct ChatGptCompactSource {
    provider_id: String,
    model: String,
    auth_provider: Arc<dyn HttpAuthProvider>,
}

fn build_provider(
    profile_id: &str,
    config: &BuiltInProviderConfig,
) -> Result<BuiltProvider, HostBuildError> {
    match config {
        BuiltInProviderConfig::ChatCompletions {
            provider_id,
            model,
            base_url,
            api_key_env,
            headers,
            extra_body,
            max_completion_tokens,
        } => {
            let mut provider = ChatCompletionsProviderConfig::new(
                provider_id.clone().unwrap_or_else(|| profile_id.into()),
                model,
            );
            if let Some(base_url) = base_url {
                provider = provider.base_url(base_url);
            }
            if let Some(api_key_env) = api_key_env {
                provider = provider.api_key_env(api_key_env);
            }
            for (name, value) in headers {
                provider = provider.header(name, value);
            }
            for (name, value) in extra_body {
                provider = provider.extra_body(name, value.clone());
            }
            if let Some(max_completion_tokens) = max_completion_tokens {
                provider = provider.max_completion_tokens(*max_completion_tokens);
            }
            Ok(BuiltProvider::model(Arc::new(
                ChatCompletionsProvider::new(provider)?,
            )))
        }
        BuiltInProviderConfig::Responses {
            provider_id,
            model,
            base_url,
            api_key_env,
            headers,
            extra_body,
            max_output_tokens,
        } => {
            let mut provider = ResponsesApiProviderConfig::new(
                provider_id.clone().unwrap_or_else(|| profile_id.into()),
                model,
            );
            if let Some(base_url) = base_url {
                provider = provider.base_url(base_url);
            }
            if let Some(api_key_env) = api_key_env {
                provider = provider.api_key_env(api_key_env);
            }
            for (name, value) in headers {
                provider = provider.header(name, value);
            }
            for (name, value) in extra_body {
                provider = provider.extra_body(name, value.clone());
            }
            if let Some(max_output_tokens) = max_output_tokens {
                provider = provider.max_output_tokens(*max_output_tokens);
            }
            Ok(BuiltProvider::model(Arc::new(ResponsesApiProvider::new(
                provider,
            )?)))
        }
        BuiltInProviderConfig::AnthropicMessages {
            provider_id,
            model,
            base_url,
            api_key_env,
            headers,
            extra_body,
            max_tokens,
        } => {
            let mut provider = AnthropicMessagesProviderConfig::new(
                provider_id.clone().unwrap_or_else(|| profile_id.into()),
                model,
            );
            if let Some(base_url) = base_url {
                provider = provider.base_url(base_url);
            }
            if let Some(api_key_env) = api_key_env {
                provider = provider.api_key_env(api_key_env);
            }
            for (name, value) in headers {
                provider = provider.header(name, value);
            }
            for (name, value) in extra_body {
                provider = provider.extra_body(name, value.clone());
            }
            if let Some(max_tokens) = max_tokens {
                provider = provider.max_tokens(*max_tokens);
            }
            Ok(BuiltProvider::model(Arc::new(
                AnthropicMessagesProvider::new(provider)?,
            )))
        }
        BuiltInProviderConfig::ChatgptResponses {
            provider_id,
            model,
            auth,
        } => {
            let provider_id = provider_id.clone().unwrap_or_else(|| profile_id.into());
            let auth_provider = build_chatgpt_auth_provider(auth)?;
            let provider = noloong_openai::provider::chatgpt_responses_provider(
                provider_id.clone(),
                model,
                Arc::clone(&auth_provider),
            )?;
            Ok(BuiltProvider {
                provider: Arc::new(provider),
                chatgpt_compact: Some(ChatGptCompactSource {
                    provider_id,
                    model: model.clone(),
                    auth_provider,
                }),
            })
        }
    }
}

fn build_chatgpt_auth_provider(
    config: &ChatGptAuthConfig,
) -> Result<Arc<dyn HttpAuthProvider>, HostBuildError> {
    match config {
        ChatGptAuthConfig::TokenFile {
            token_file,
            token_file_env,
        } => {
            let token_file =
                resolve_chatgpt_token_file(token_file.as_deref(), token_file_env.as_deref())?;
            let storage =
                Arc::new(ChatGptTokenStorage::file(token_file)) as Arc<dyn ChatGptTokenStore>;
            Ok(Arc::new(ChatGptAuthManager::new(storage)))
        }
        ChatGptAuthConfig::EnvHeaders { id, headers } => Ok(Arc::new(
            EnvHttpAuthProvider::from_env_headers(id.clone(), headers.clone()),
        )),
    }
}

fn build_profile_compaction(
    profile_id: &str,
    config: &ProfileCompactionConfig,
    chatgpt_source: Option<&ChatGptCompactSource>,
) -> Result<Option<RuntimeCompaction>, HostBuildError> {
    match config {
        ProfileCompactionConfig::Auto => chatgpt_source
            .map(|source| {
                openai_responses_runtime_compaction(
                    profile_id,
                    source,
                    OpenAiResponsesCompactionOptions::default(),
                )
            })
            .transpose(),
        ProfileCompactionConfig::None => Ok(None),
        ProfileCompactionConfig::OpenaiResponses {
            id,
            model,
            context_window_tokens,
            reserve_tokens,
            keep_recent_tokens,
            mode,
            request_timeout_secs,
        } => {
            let source = chatgpt_source.ok_or_else(|| {
                CliConfigError::ParseConfig(
                    "openai_responses compaction requires a chatgpt_responses provider".into(),
                )
            })?;
            openai_responses_runtime_compaction(
                profile_id,
                source,
                OpenAiResponsesCompactionOptions {
                    id: id.clone(),
                    model: model.clone(),
                    context_window_tokens: *context_window_tokens,
                    reserve_tokens: *reserve_tokens,
                    keep_recent_tokens: *keep_recent_tokens,
                    mode: *mode,
                    request_timeout_secs: *request_timeout_secs,
                },
            )
            .map(Some)
        }
    }
}

#[derive(Clone, Debug, Default)]
struct OpenAiResponsesCompactionOptions {
    id: Option<String>,
    model: Option<String>,
    context_window_tokens: Option<u64>,
    reserve_tokens: Option<u64>,
    keep_recent_tokens: Option<u64>,
    mode: Option<ContextCompactionMode>,
    request_timeout_secs: Option<u64>,
}

fn openai_responses_runtime_compaction(
    profile_id: &str,
    source: &ChatGptCompactSource,
    options: OpenAiResponsesCompactionOptions,
) -> Result<RuntimeCompaction, HostBuildError> {
    let reserve_tokens = options
        .reserve_tokens
        .unwrap_or(DEFAULT_CHATGPT_COMPACTION_RESERVE_TOKENS);
    let context_window_tokens = options
        .context_window_tokens
        .unwrap_or(DEFAULT_CHATGPT_COMPACTION_TRIGGER_TOKENS.saturating_add(reserve_tokens));
    let keep_recent_tokens = options
        .keep_recent_tokens
        .unwrap_or(DEFAULT_CHATGPT_COMPACTION_KEEP_RECENT_TOKENS);
    let context_config = ContextCompactionConfig::new(context_window_tokens)
        .reserve_tokens(reserve_tokens)
        .keep_recent_tokens(keep_recent_tokens)
        .mode(options.mode.unwrap_or_default())
        .metadata("source", json!("openai_responses"))
        .metadata("profileId", json!(profile_id))
        .metadata("providerId", json!(source.provider_id.clone()));
    let compactor_id = options
        .id
        .unwrap_or_else(|| format!("{}.compact", source.provider_id));
    let model = options.model.unwrap_or_else(|| source.model.clone());
    let mut compactor_config = OpenAiResponsesCompactorConfig::new(compactor_id, model)
        .auth_provider(Arc::clone(&source.auth_provider));
    if let Some(request_timeout_secs) = options.request_timeout_secs {
        if request_timeout_secs == 0 {
            return Err(CliConfigError::ParseConfig(
                "compaction requestTimeoutSecs must be greater than zero".into(),
            )
            .into());
        }
        compactor_config =
            compactor_config.request_timeout(Duration::from_secs(request_timeout_secs));
    }
    let compactor = OpenAiResponsesCompactor::new(compactor_config)?;
    Ok(RuntimeCompaction {
        config: context_config,
        compactor: Arc::new(compactor),
    })
}

#[derive(Clone)]
struct EnvHttpAuthProvider {
    id: String,
    headers: Vec<EnvAuthHeader>,
}

impl EnvHttpAuthProvider {
    fn from_env_headers(id: String, headers: Vec<EnvHeaderConfig>) -> Self {
        Self {
            id,
            headers: headers
                .into_iter()
                .map(|header| EnvAuthHeader {
                    name: header.name,
                    env: header.env,
                    value_prefix: header.value_prefix,
                })
                .collect(),
        }
    }
}

impl HttpAuthProvider for EnvHttpAuthProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn headers<'a>(
        &'a self,
        _context: HttpAuthContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, HttpAuthHeaders> {
        Box::pin(async move {
            let headers = self
                .headers
                .iter()
                .map(|header| {
                    let value = env::var(&header.env).map_err(|_| {
                        AgentCoreError::Provider(format!("missing env {}", header.env))
                    })?;
                    Ok(HttpAuthHeader::new(
                        &header.name,
                        format!("{}{}", header.value_prefix.as_deref().unwrap_or(""), value),
                    ))
                })
                .collect::<Result<Vec<_>, AgentCoreError>>()?;
            Ok(HttpAuthHeaders::new(headers))
        })
    }
}

#[derive(Clone)]
struct EnvAuthHeader {
    name: String,
    env: String,
    value_prefix: Option<String>,
}

#[derive(Debug, Error)]
pub enum HostBuildError {
    #[error("{0}")]
    Config(#[from] CliConfigError),
    #[error("interaction host failed: {0}")]
    Interaction(#[from] InteractionError),
    #[error("agent core failed: {0}")]
    Core(#[from] AgentCoreError),
}

fn opendal_error(error: opendal::Error) -> HostBuildError {
    HostBuildError::Config(CliConfigError::ParseConfig(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_CHATGPT_COMPACTION_TRIGGER_TOKENS, RuntimeProfile, build_registry};
    use crate::config::{HostProfileConfig, RuntimeProfileConfig};
    use noloong_agent::{
        AgentManifest, AgentSession, ManifestPatch, interaction::AgentRuntimeProfile,
    };
    use noloong_agent_core::ContextCompactionMode;

    #[tokio::test]
    async fn profile_config_builds_registry_store() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "registryStore": {"type": "memory"},
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "chat_completions", "model": "gpt-5.5-mini"}
                }]
            }"#,
        )
        .unwrap();

        let registry = build_registry(&config).await.unwrap();

        assert_eq!(registry.profile_descriptors()[0].profile_id, "default");
    }

    #[tokio::test]
    async fn example_telegram_openrouter_free_profile_builds_registry() {
        let config = serde_json::from_str::<HostProfileConfig>(include_str!(
            "../examples/profile-configs/telegram-openrouter-free.json"
        ))
        .unwrap();

        let registry = build_registry(&config).await.unwrap();

        assert_eq!(
            registry.profile_descriptors()[0].profile_id,
            "telegram-openrouter-free"
        );
    }

    #[tokio::test]
    async fn example_chatgpt_codex_subscription_profile_builds_registry() {
        let config = serde_json::from_str::<HostProfileConfig>(include_str!(
            "../examples/profile-configs/chatgpt-codex-subscription.json"
        ))
        .unwrap();

        let registry = build_registry(&config).await.unwrap();

        assert_eq!(
            registry.profile_descriptors()[0].profile_id,
            "chatgpt-codex"
        );
    }

    #[tokio::test]
    async fn example_plugin_stdio_profile_builds_registry() {
        let config = serde_json::from_str::<HostProfileConfig>(include_str!(
            "../examples/profile-configs/plugin-stdio-example.json"
        ))
        .unwrap();

        let registry = build_registry(&config).await.unwrap();

        assert_eq!(
            registry.profile_descriptors()[0].profile_id,
            "plugin-stdio-example"
        );
    }

    #[test]
    fn profile_default_plugins_become_default_manifest_patches() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {"type": "responses", "model": "gpt-5.5-mini"},
                "plugins": [{
                    "pluginId": "echo",
                    "displayName": "Echo",
                    "transport": {
                        "type": "stdio",
                        "command": "node",
                        "args": ["examples/extensions/echo.mjs"]
                    },
                    "allowedCapabilities": [
                        {"type": "tool", "name": "echo.run"}
                    ]
                }],
                "manifestPatches": [{
                    "op": "set_plugin_enabled",
                    "pluginId": "echo",
                    "enabled": true
                }]
            }"#,
        )
        .unwrap();

        let profile = RuntimeProfile::try_from_config(&config).unwrap();
        let descriptor = profile.descriptor();

        assert!(matches!(
            &descriptor.default_manifest_patches[0],
            ManifestPatch::RegisterPlugin { plugin } if plugin.plugin_id == "echo"
        ));
        assert!(matches!(
            &descriptor.default_manifest_patches[1],
            ManifestPatch::SetPluginEnabled { plugin_id, enabled }
                if plugin_id == "echo" && *enabled
        ));
    }

    #[tokio::test]
    async fn chatgpt_profile_auto_compaction_registers_codex_compactor() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "chatgpt",
                "displayName": "ChatGPT",
                "provider": {"type": "chatgpt_responses", "model": "gpt-5.4-mini"}
            }"#,
        )
        .unwrap();
        let profile = RuntimeProfile::try_from_config(&config).unwrap();
        let session = AgentSession::builder().build();
        let manifest = AgentManifest::default();

        let runtime = profile.build_runtime(&session, &manifest).await.unwrap();
        let compaction = runtime
            .context_compaction_config()
            .expect("auto ChatGPT profile should register compaction");

        assert_eq!(
            compaction.trigger_threshold(),
            DEFAULT_CHATGPT_COMPACTION_TRIGGER_TOKENS
        );
        assert_eq!(compaction.mode, ContextCompactionMode::PersistentState);
        assert_eq!(compaction.metadata["source"], "openai_responses");
        assert_eq!(compaction.metadata["profileId"], "chatgpt");
    }

    #[tokio::test]
    async fn chatgpt_profile_can_disable_auto_compaction() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "chatgpt",
                "displayName": "ChatGPT",
                "provider": {"type": "chatgpt_responses", "model": "gpt-5.4-mini"},
                "compaction": {"type": "none"}
            }"#,
        )
        .unwrap();
        let profile = RuntimeProfile::try_from_config(&config).unwrap();
        let session = AgentSession::builder().build();
        let manifest = AgentManifest::default();

        let runtime = profile.build_runtime(&session, &manifest).await.unwrap();

        assert!(runtime.context_compaction_config().is_none());
    }

    #[tokio::test]
    async fn chatgpt_profile_openai_responses_compaction_honors_overrides() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "chatgpt",
                "displayName": "ChatGPT",
                "provider": {"type": "chatgpt_responses", "model": "gpt-5.4-mini"},
                "compaction": {
                    "type": "openai_responses",
                    "contextWindowTokens": 200000,
                    "reserveTokens": 32000,
                    "keepRecentTokens": 64000,
                    "mode": "request_only",
                    "requestTimeoutSecs": 120
                }
            }"#,
        )
        .unwrap();
        let profile = RuntimeProfile::try_from_config(&config).unwrap();
        let session = AgentSession::builder().build();
        let manifest = AgentManifest::default();

        let runtime = profile.build_runtime(&session, &manifest).await.unwrap();
        let compaction = runtime
            .context_compaction_config()
            .expect("explicit compaction should be registered");

        assert_eq!(compaction.context_window_tokens, 200_000);
        assert_eq!(compaction.reserve_tokens, 32_000);
        assert_eq!(compaction.keep_recent_tokens, 64_000);
        assert_eq!(compaction.mode, ContextCompactionMode::RequestOnly);
    }
}

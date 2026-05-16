use crate::config::{
    AnthropicProviderReasoningConfig, AnthropicProviderThinkingMode, BuiltInProviderConfig,
    ChatCompletionsReasoningConfig, ChatGptAuthConfig, CliConfigError, EnvHeaderConfig,
    HostProfileConfig, ProfileCompactionConfig, ProfileEventStoreConfig, RegistryStoreConfig,
    ResponsesProviderReasoningConfig, RuntimeProfileConfig, ensure_sqlite_database_parent,
    resolve_chatgpt_token_file, resolve_state_database_url,
    validate_responses_reasoning_state_mode,
};
use crate::models_dev::ModelsDevRegistry;
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
    ContextCompactionConfig, ContextCompactionMode, ContextCompactor, EventStore, HttpAuthContext,
    HttpAuthHeader, HttpAuthHeaders, HttpAuthProvider, InMemoryEventStore, ModelProvider,
    ResponsesApiProvider, ResponsesApiProviderConfig, ResponsesReasoningConfig, ResponsesStateMode,
    SqliteEventStore, SqliteEventStoreConfig,
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

const DEFAULT_CHATGPT_COMPACTION_INPUT_LIMIT_TOKENS: u64 = 144_384;
const MODELS_DEV_OPENAI_PROVIDER_ID: &str = "openai";

pub async fn build_registry(
    config: &HostProfileConfig,
) -> Result<AgentSessionRegistry, HostBuildError> {
    config.validate()?;
    let state_database_url = if config_uses_default_state_database(config) {
        let state_database_url = resolve_state_database_url()?;
        ensure_sqlite_database_parent(&state_database_url)?;
        Some(state_database_url)
    } else {
        None
    };
    build_registry_with_optional_state_database_url(config, state_database_url.as_deref()).await
}

#[cfg(test)]
async fn build_registry_with_state_database_url(
    config: &HostProfileConfig,
    state_database_url: &str,
) -> Result<AgentSessionRegistry, HostBuildError> {
    build_registry_with_optional_state_database_url(config, Some(state_database_url)).await
}

async fn build_registry_with_optional_state_database_url(
    config: &HostProfileConfig,
    state_database_url: Option<&str>,
) -> Result<AgentSessionRegistry, HostBuildError> {
    config.validate()?;
    let models_dev = if config.profiles.iter().any(profile_needs_models_dev) {
        let models_dev = ModelsDevRegistry::load_default().await;
        models_dev.refresh_cache_in_background();
        Some(models_dev)
    } else {
        None
    };
    let default_event_store = if config
        .profiles
        .iter()
        .any(|profile| profile.event_store.is_none())
    {
        Some(build_event_store(None, state_database_url).await?)
    } else {
        None
    };
    let mut profiles = Vec::with_capacity(config.profiles.len());
    for profile_config in &config.profiles {
        profiles.push(Arc::new(
            RuntimeProfile::try_from_config(
                profile_config,
                models_dev.as_ref(),
                default_event_store.clone(),
                state_database_url,
            )
            .await?,
        ) as Arc<dyn AgentRuntimeProfile>);
    }
    let default_profile_id = config
        .default_profile_id
        .clone()
        .unwrap_or_else(|| config.profiles[0].profile_id.clone());
    let store = build_registry_store(config.registry_store.as_ref(), state_database_url).await?;
    AgentSessionRegistry::with_store(default_profile_id, profiles, store)
        .map_err(HostBuildError::Interaction)
}

fn config_uses_default_state_database(config: &HostProfileConfig) -> bool {
    config.registry_store.is_none()
        || config
            .profiles
            .iter()
            .any(|profile| profile.event_store.is_none())
}

fn profile_needs_models_dev(config: &RuntimeProfileConfig) -> bool {
    if !matches!(
        config.provider,
        BuiltInProviderConfig::ChatgptResponses { .. }
    ) {
        return false;
    }
    match &config.compaction {
        ProfileCompactionConfig::Auto => true,
        ProfileCompactionConfig::OpenaiResponses {
            input_limit_tokens, ..
        } => input_limit_tokens.is_none(),
        ProfileCompactionConfig::None => false,
    }
}

async fn build_registry_store(
    config: Option<&RegistryStoreConfig>,
    state_database_url: Option<&str>,
) -> Result<Arc<dyn AgentSessionRegistryStore>, HostBuildError> {
    match config {
        None => {
            let state_database_url = required_state_database_url(state_database_url)?;
            let store = SqlAgentSessionRegistryStore::connect(
                SqlAgentSessionRegistryStoreConfig::new(state_database_url),
            )
            .await?;
            Ok(Arc::new(store))
        }
        Some(RegistryStoreConfig::Memory) => {
            Ok(Arc::new(InMemoryAgentSessionRegistryStore::default()))
        }
        Some(RegistryStoreConfig::Sqlite { database_url }) => {
            ensure_sqlite_database_parent(database_url)?;
            let store = SqlAgentSessionRegistryStore::connect(
                SqlAgentSessionRegistryStoreConfig::new(database_url),
            )
            .await?;
            Ok(Arc::new(store))
        }
        Some(RegistryStoreConfig::Postgres { database_url }) => {
            let store = SqlAgentSessionRegistryStore::connect(
                SqlAgentSessionRegistryStoreConfig::new(database_url),
            )
            .await?;
            Ok(Arc::new(store))
        }
        Some(RegistryStoreConfig::ObjectMemory { prefix }) => {
            Ok(Arc::new(OpenDalAgentSessionRegistryStore::new(
                Operator::new(Memory::default())
                    .map_err(opendal_error)?
                    .finish(),
                OpenDalAgentSessionRegistryStoreConfig::new(prefix),
            )))
        }
        Some(RegistryStoreConfig::ObjectFs { root, prefix }) => {
            let builder = Fs::default().root(root);
            Ok(Arc::new(OpenDalAgentSessionRegistryStore::new(
                Operator::new(builder).map_err(opendal_error)?.finish(),
                OpenDalAgentSessionRegistryStoreConfig::new(prefix),
            )))
        }
    }
}

async fn build_event_store(
    config: Option<&ProfileEventStoreConfig>,
    state_database_url: Option<&str>,
) -> Result<Arc<dyn EventStore>, HostBuildError> {
    match config {
        None => {
            let state_database_url = required_state_database_url(state_database_url)?;
            let store =
                SqliteEventStore::connect(SqliteEventStoreConfig::new(state_database_url)).await?;
            Ok(Arc::new(store))
        }
        Some(ProfileEventStoreConfig::Memory) => Ok(Arc::new(InMemoryEventStore::new())),
        Some(ProfileEventStoreConfig::Sqlite {
            database_url,
            migrate_on_connect,
        }) => {
            ensure_sqlite_database_parent(database_url)?;
            let mut config = SqliteEventStoreConfig::new(database_url);
            if !*migrate_on_connect {
                config = config.without_migrations();
            }
            let store = SqliteEventStore::connect(config).await?;
            Ok(Arc::new(store))
        }
    }
}

fn required_state_database_url(state_database_url: Option<&str>) -> Result<&str, HostBuildError> {
    state_database_url.ok_or_else(|| {
        HostBuildError::Config(CliConfigError::ParseConfig(
            "profile config omitted registryStore or eventStore but no state database URL was resolved"
                .into(),
        ))
    })
}

#[derive(Clone)]
struct RuntimeProfile {
    descriptor: InteractionProfileDescriptor,
    provider: Arc<dyn ModelProvider>,
    event_store: Arc<dyn EventStore>,
    compaction: Option<RuntimeCompaction>,
}

impl RuntimeProfile {
    async fn try_from_config(
        config: &RuntimeProfileConfig,
        models_dev: Option<&ModelsDevRegistry>,
        default_event_store: Option<Arc<dyn EventStore>>,
        state_database_url: Option<&str>,
    ) -> Result<Self, HostBuildError> {
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
            models_dev,
        )?;
        let event_store = match config.event_store.as_ref() {
            Some(event_store_config) => {
                build_event_store(Some(event_store_config), state_database_url).await?
            }
            None => match default_event_store {
                Some(event_store) => event_store,
                None => build_event_store(None, state_database_url).await?,
            },
        };
        Ok(Self {
            descriptor: InteractionProfileDescriptor {
                profile_id: config.profile_id.clone(),
                display_name: config.display_name.clone(),
                description: config.description.clone(),
                default_manifest_patches,
                metadata: config.metadata.clone(),
            },
            provider: provider.provider,
            event_store,
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
                .with_event_store(Arc::clone(&self.event_store))
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
    state_mode: ResponsesStateMode,
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
            reasoning,
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
            provider = apply_chat_completions_reasoning(provider, reasoning.as_ref());
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
            state_mode,
            reasoning,
            allow_file_data_url_input,
        } => {
            let mut provider = ResponsesApiProviderConfig::new(
                provider_id.clone().unwrap_or_else(|| profile_id.into()),
                model,
            )
            .with_state_mode(*state_mode);
            if *allow_file_data_url_input {
                provider = provider.allow_file_data_url_input(true);
            }
            if let Some(base_url) = base_url {
                provider = provider.base_url(base_url);
            }
            if let Some(api_key_env) = api_key_env {
                provider = provider.api_key_env(api_key_env);
            }
            for (name, value) in headers {
                provider = provider.header(name, value);
            }
            provider = apply_responses_reasoning(provider, reasoning.as_ref(), *state_mode)?;
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
            reasoning,
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
            provider = apply_anthropic_reasoning(provider, reasoning.as_ref());
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
            state_mode,
            reasoning,
            allow_file_data_url_input,
        } => {
            let provider_id = provider_id.clone().unwrap_or_else(|| profile_id.into());
            let auth_provider = build_chatgpt_auth_provider(auth)?;
            let mut provider_config = noloong_openai::provider::chatgpt_responses_provider_config(
                provider_id.clone(),
                model,
                Arc::clone(&auth_provider),
            )
            .with_state_mode(*state_mode);
            if *allow_file_data_url_input {
                provider_config = provider_config.allow_file_data_url_input(true);
            }
            let provider_config =
                apply_responses_reasoning(provider_config, reasoning.as_ref(), *state_mode)?;
            let provider = ResponsesApiProvider::new(provider_config)?;
            Ok(BuiltProvider {
                provider: Arc::new(provider),
                chatgpt_compact: Some(ChatGptCompactSource {
                    provider_id,
                    model: model.clone(),
                    auth_provider,
                    state_mode: *state_mode,
                }),
            })
        }
    }
}

fn apply_chat_completions_reasoning(
    mut provider: ChatCompletionsProviderConfig,
    reasoning: Option<&ChatCompletionsReasoningConfig>,
) -> ChatCompletionsProviderConfig {
    let Some(reasoning) = reasoning else {
        return provider;
    };
    for (name, value) in chat_completions_reasoning_extra_body(reasoning) {
        provider = provider.extra_body(name, value);
    }
    provider
}

fn chat_completions_reasoning_extra_body(
    reasoning: &ChatCompletionsReasoningConfig,
) -> serde_json::Map<String, serde_json::Value> {
    let enabled = reasoning.enabled;
    let mut body = serde_json::Map::new();
    body.insert("enable_thinking".into(), json!(enabled));
    body.insert(
        "thinking".into(),
        json!({
            "type": if enabled { "enabled" } else { "disabled" },
        }),
    );
    body.insert("reasoning".into(), json!({ "enabled": enabled }));
    body.insert("reasoning_split".into(), json!(enabled));
    body.insert(
        "chat_template_kwargs".into(),
        json!({
            "enable_thinking": enabled,
        }),
    );
    if enabled && let Some(effort) = reasoning.effort {
        body.insert("reasoning_effort".into(), json!(effort.as_str()));
    }
    body
}

fn apply_responses_reasoning(
    mut provider: ResponsesApiProviderConfig,
    reasoning: Option<&ResponsesProviderReasoningConfig>,
    state_mode: ResponsesStateMode,
) -> Result<ResponsesApiProviderConfig, HostBuildError> {
    let Some(reasoning) = reasoning else {
        return Ok(provider);
    };
    validate_responses_reasoning_state_mode(state_mode, Some(reasoning))?;
    if let Some(core_reasoning) = responses_reasoning_config(reasoning) {
        provider = provider.reasoning(core_reasoning);
    }
    if reasoning.enabled
        && (reasoning.include_encrypted.unwrap_or(false) || state_mode.is_stateless())
    {
        provider = provider.include_encrypted_reasoning(true);
    }
    Ok(provider)
}

fn responses_reasoning_config(
    reasoning: &ResponsesProviderReasoningConfig,
) -> Option<ResponsesReasoningConfig> {
    if !reasoning.enabled || (reasoning.effort.is_none() && reasoning.summary.is_none()) {
        return None;
    }
    let mut config = ResponsesReasoningConfig::new();
    if let Some(effort) = reasoning.effort {
        config = config.effort(effort.into());
    }
    if let Some(summary) = reasoning.summary {
        config = config.summary(summary.into());
    }
    Some(config)
}

fn apply_anthropic_reasoning(
    mut provider: AnthropicMessagesProviderConfig,
    reasoning: Option<&AnthropicProviderReasoningConfig>,
) -> AnthropicMessagesProviderConfig {
    let Some(reasoning) = reasoning else {
        return provider;
    };
    if let Some(effort) = reasoning.effort {
        provider = provider.output_effort(effort.into());
    }
    match reasoning.thinking {
        Some(AnthropicProviderThinkingMode::Adaptive) => provider.adaptive_thinking(),
        Some(AnthropicProviderThinkingMode::Disabled) => provider.disable_thinking(),
        Some(AnthropicProviderThinkingMode::Omit) | None => provider,
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
    models_dev: Option<&ModelsDevRegistry>,
) -> Result<Option<RuntimeCompaction>, HostBuildError> {
    match config {
        ProfileCompactionConfig::Auto => chatgpt_source
            .map(|source| {
                openai_responses_runtime_compaction(
                    profile_id,
                    source,
                    OpenAiResponsesCompactionOptions::default(),
                    models_dev,
                )
            })
            .transpose(),
        ProfileCompactionConfig::None => Ok(None),
        ProfileCompactionConfig::OpenaiResponses {
            id,
            input_limit_model,
            compact_model,
            input_limit_tokens,
            trigger_ratio,
            summary_budget_tokens,
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
                    input_limit_model: input_limit_model.clone(),
                    compact_model: compact_model.clone(),
                    input_limit_tokens: *input_limit_tokens,
                    trigger_ratio: *trigger_ratio,
                    summary_budget_tokens: *summary_budget_tokens,
                    keep_recent_tokens: *keep_recent_tokens,
                    mode: *mode,
                    request_timeout_secs: *request_timeout_secs,
                },
                models_dev,
            )
            .map(Some)
        }
    }
}

#[derive(Clone, Debug, Default)]
struct OpenAiResponsesCompactionOptions {
    id: Option<String>,
    input_limit_model: Option<String>,
    compact_model: Option<String>,
    input_limit_tokens: Option<u64>,
    trigger_ratio: Option<f64>,
    summary_budget_tokens: Option<u64>,
    keep_recent_tokens: Option<u64>,
    mode: Option<ContextCompactionMode>,
    request_timeout_secs: Option<u64>,
}

fn openai_responses_runtime_compaction(
    profile_id: &str,
    source: &ChatGptCompactSource,
    options: OpenAiResponsesCompactionOptions,
    models_dev: Option<&ModelsDevRegistry>,
) -> Result<RuntimeCompaction, HostBuildError> {
    let input_limit_model = options
        .input_limit_model
        .unwrap_or_else(|| source.model.clone());
    let input_limit_tokens = options.input_limit_tokens.unwrap_or_else(|| {
        models_dev
            .and_then(|registry| {
                registry.input_limit(MODELS_DEV_OPENAI_PROVIDER_ID, &input_limit_model)
            })
            .unwrap_or(DEFAULT_CHATGPT_COMPACTION_INPUT_LIMIT_TOKENS)
    });
    let mut context_config = ContextCompactionConfig::new(input_limit_tokens);
    if let Some(trigger_ratio) = options.trigger_ratio {
        context_config = context_config.trigger_ratio(trigger_ratio);
    }
    if let Some(summary_budget_tokens) = options.summary_budget_tokens {
        context_config = context_config.summary_budget_tokens(summary_budget_tokens);
    }
    if let Some(keep_recent_tokens) = options.keep_recent_tokens {
        context_config = context_config.keep_recent_tokens(keep_recent_tokens);
    }
    let context_config = context_config
        .mode(options.mode.unwrap_or_default())
        .metadata("source", json!("openai_responses"))
        .metadata("profileId", json!(profile_id))
        .metadata("providerId", json!(source.provider_id.clone()))
        .metadata("inputLimitProvider", json!(MODELS_DEV_OPENAI_PROVIDER_ID))
        .metadata("inputLimitModel", json!(input_limit_model));
    context_config.validate()?;
    let compactor_id = options
        .id
        .unwrap_or_else(|| format!("{}.compact", source.provider_id));
    let model = options
        .compact_model
        .unwrap_or_else(|| source.model.clone());
    let context_config = context_config.metadata("compactModel", json!(model.clone()));
    let mut compactor_config = OpenAiResponsesCompactorConfig::new(compactor_id, model)
        .auth_provider(Arc::clone(&source.auth_provider))
        .state_mode(source.state_mode);
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
    use super::{
        HostBuildError, RuntimeProfile, apply_anthropic_reasoning,
        apply_chat_completions_reasoning, apply_responses_reasoning,
        build_registry_with_optional_state_database_url, build_registry_with_state_database_url,
        chat_completions_reasoning_extra_body, config_uses_default_state_database,
        responses_reasoning_config,
    };
    use crate::config::{
        AnthropicProviderReasoningConfig, AnthropicProviderReasoningEffort,
        AnthropicProviderThinkingMode, BuiltInProviderConfig, ChatCompletionsReasoningConfig,
        ChatCompletionsReasoningEffort, HostProfileConfig, ProfileEventStoreConfig,
        ResponsesProviderReasoningConfig, ResponsesProviderReasoningEffort,
        ResponsesProviderReasoningSummary, RuntimeProfileConfig,
    };
    use crate::models_dev::ModelsDevRegistry;
    use noloong_agent::{
        AgentManifest, AgentSession, ManifestPatch,
        interaction::{AgentRuntimeProfile, AgentSessionCreateRequest},
    };
    use noloong_agent_core::{
        AgentEvent, AgentEventKind, AnthropicEffort, AnthropicMessagesProviderConfig,
        AnthropicThinkingConfig, BoxFuture, CancellationToken, ChatCompletionsProviderConfig,
        ContextCompactionMode, EventStore as _, ModelProvider, ModelRequest, ModelStreamEvent,
        ModelStreamSink, ResponsesApiProviderConfig, ResponsesReasoningEffort,
        ResponsesReasoningSummary, ResponsesStateMode, SqliteEventStore, SqliteEventStoreConfig,
        StopReason,
    };
    use std::{
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    static NEXT_DB_ID: AtomicU64 = AtomicU64::new(0);

    async fn runtime_profile(
        config: &RuntimeProfileConfig,
    ) -> Result<RuntimeProfile, HostBuildError> {
        let models_dev = models_dev_registry();
        RuntimeProfile::try_from_config(config, Some(&models_dev), None, Some("sqlite::memory:"))
            .await
    }

    fn models_dev_registry() -> ModelsDevRegistry {
        ModelsDevRegistry::from_json_for_tests(
            r#"{
                "openai": {
                    "models": {
                        "gpt-5.4-mini": {
                            "limit": {"context": 400000, "input": 272000, "output": 128000}
                        },
                        "compact-only": {
                            "limit": {"context": 64000, "input": 32000, "output": 8192}
                        }
                    }
                }
            }"#,
        )
    }

    #[tokio::test]
    async fn profile_config_builds_registry_store() {
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "registryStore": {"type": "memory"},
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "chat_completions", "model": "gpt-5.4-mini"},
                    "eventStore": {"type": "memory"}
                }]
            }"#,
        )
        .unwrap();

        assert!(!config_uses_default_state_database(&config));
        let registry = build_registry_with_optional_state_database_url(&config, None)
            .await
            .unwrap();

        assert_eq!(registry.profile_descriptors()[0].profile_id, "default");
    }

    #[tokio::test]
    async fn profile_config_omits_registry_store_and_uses_state_database() {
        let db = TempSqliteDb::new("default-registry-store");
        let config = serde_json::from_str::<HostProfileConfig>(
            r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "chat_completions", "model": "gpt-5.4-mini"}
                }]
            }"#,
        )
        .unwrap();
        let database_url = sqlite_database_url(&db.path);

        let first_registry = build_registry_with_state_database_url(&config, &database_url)
            .await
            .unwrap();
        first_registry
            .create_session(AgentSessionCreateRequest {
                session_id: Some("root".into()),
                ..AgentSessionCreateRequest::default()
            })
            .await
            .unwrap();
        drop(first_registry);

        let second_registry = build_registry_with_state_database_url(&config, &database_url)
            .await
            .unwrap();
        let descriptor = second_registry
            .get_descriptor("root")
            .await
            .unwrap()
            .expect("session restored from default sqlite registry store");

        assert_eq!(descriptor.session_id, "root");
    }

    #[test]
    fn chat_completions_reasoning_body_enables_common_switches_and_effort() {
        let body = chat_completions_reasoning_extra_body(&ChatCompletionsReasoningConfig {
            enabled: true,
            effort: Some(ChatCompletionsReasoningEffort::XHigh),
        });

        assert_eq!(body["enable_thinking"], serde_json::json!(true));
        assert_eq!(body["thinking"], serde_json::json!({ "type": "enabled" }));
        assert_eq!(body["reasoning"], serde_json::json!({ "enabled": true }));
        assert_eq!(body["reasoning_split"], serde_json::json!(true));
        assert_eq!(
            body["chat_template_kwargs"],
            serde_json::json!({ "enable_thinking": true })
        );
        assert_eq!(body["reasoning_effort"], "xhigh");
    }

    #[test]
    fn chat_completions_reasoning_body_disables_common_switches_without_effort() {
        let body = chat_completions_reasoning_extra_body(&ChatCompletionsReasoningConfig {
            enabled: false,
            effort: Some(ChatCompletionsReasoningEffort::High),
        });

        assert_eq!(body["enable_thinking"], serde_json::json!(false));
        assert_eq!(body["thinking"], serde_json::json!({ "type": "disabled" }));
        assert_eq!(body["reasoning"], serde_json::json!({ "enabled": false }));
        assert_eq!(body["reasoning_split"], serde_json::json!(false));
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn chat_completions_extra_body_can_override_reasoning_mapping() {
        let provider = apply_chat_completions_reasoning(
            ChatCompletionsProviderConfig::new("openrouter", "openrouter/free"),
            Some(&ChatCompletionsReasoningConfig {
                enabled: true,
                effort: Some(ChatCompletionsReasoningEffort::High),
            }),
        )
        .extra_body("reasoning_effort", serde_json::json!("low"))
        .extra_body("reasoning", serde_json::json!({ "enabled": false }));

        assert_eq!(provider.extra_body["reasoning_effort"], "low");
        assert_eq!(
            provider.extra_body["reasoning"],
            serde_json::json!({ "enabled": false })
        );
    }

    #[test]
    fn responses_reasoning_maps_fields_and_respects_disabled() {
        let reasoning = responses_reasoning_config(&ResponsesProviderReasoningConfig {
            enabled: true,
            effort: Some(ResponsesProviderReasoningEffort::High),
            summary: Some(ResponsesProviderReasoningSummary::Concise),
            include_encrypted: Some(true),
        })
        .expect("enabled reasoning config should render");
        let disabled = responses_reasoning_config(&ResponsesProviderReasoningConfig {
            enabled: false,
            effort: Some(ResponsesProviderReasoningEffort::High),
            summary: Some(ResponsesProviderReasoningSummary::Concise),
            include_encrypted: Some(true),
        });

        assert_eq!(reasoning.effort, Some(ResponsesReasoningEffort::High));
        assert_eq!(reasoning.summary, Some(ResponsesReasoningSummary::Concise));
        assert!(disabled.is_none());
    }

    #[test]
    fn responses_reasoning_sets_encrypted_include_only_when_enabled() {
        let enabled = apply_responses_reasoning(
            ResponsesApiProviderConfig::new("openai", "gpt-5.4-mini"),
            Some(&ResponsesProviderReasoningConfig {
                enabled: true,
                effort: None,
                summary: None,
                include_encrypted: Some(true),
            }),
            ResponsesStateMode::Stateless,
        )
        .unwrap();
        let disabled = apply_responses_reasoning(
            ResponsesApiProviderConfig::new("openai", "gpt-5.4-mini"),
            Some(&ResponsesProviderReasoningConfig {
                enabled: false,
                effort: Some(ResponsesProviderReasoningEffort::High),
                summary: Some(ResponsesProviderReasoningSummary::Detailed),
                include_encrypted: Some(true),
            }),
            ResponsesStateMode::Stateless,
        )
        .unwrap();

        assert!(enabled.include_encrypted_reasoning);
        assert!(enabled.reasoning.is_none());
        assert!(!disabled.include_encrypted_reasoning);
        assert!(disabled.reasoning.is_none());
    }

    #[test]
    fn responses_reasoning_rejects_explicit_false_include_in_stateless_mode() {
        let error = apply_responses_reasoning(
            ResponsesApiProviderConfig::new("openai", "gpt-5.4-mini"),
            Some(&ResponsesProviderReasoningConfig {
                enabled: true,
                effort: Some(ResponsesProviderReasoningEffort::Medium),
                summary: None,
                include_encrypted: Some(false),
            }),
            ResponsesStateMode::Stateless,
        )
        .expect_err("stateless reasoning must not explicitly disable encrypted replay");

        assert!(error.to_string().contains("includeEncrypted"));
    }

    #[test]
    fn responses_reasoning_allows_explicit_false_include_in_stateful_mode() {
        let provider = apply_responses_reasoning(
            ResponsesApiProviderConfig::new("openai", "gpt-5.4-mini"),
            Some(&ResponsesProviderReasoningConfig {
                enabled: true,
                effort: Some(ResponsesProviderReasoningEffort::Medium),
                summary: None,
                include_encrypted: Some(false),
            }),
            ResponsesStateMode::Stateful,
        )
        .unwrap();

        assert!(!provider.include_encrypted_reasoning);
        assert!(provider.reasoning.is_some());
    }

    #[test]
    fn anthropic_reasoning_maps_effort_and_thinking_mode() {
        let provider = apply_anthropic_reasoning(
            AnthropicMessagesProviderConfig::new("anthropic", "claude-test"),
            Some(&AnthropicProviderReasoningConfig {
                effort: Some(AnthropicProviderReasoningEffort::XHigh),
                thinking: Some(AnthropicProviderThinkingMode::Adaptive),
            }),
        );
        let omitted = apply_anthropic_reasoning(
            AnthropicMessagesProviderConfig::new("anthropic", "claude-test"),
            Some(&AnthropicProviderReasoningConfig {
                effort: None,
                thinking: Some(AnthropicProviderThinkingMode::Omit),
            }),
        );

        assert_eq!(provider.output_effort, Some(AnthropicEffort::XHigh));
        assert_eq!(provider.thinking, Some(AnthropicThinkingConfig::Adaptive));
        assert!(omitted.output_effort.is_none());
        assert!(omitted.thinking.is_none());
    }

    #[tokio::test]
    async fn profile_config_builds_sqlite_event_store() {
        let db = TempSqliteDb::new("profile-event-store");
        let config = runtime_profile_config(sqlite_event_store(&db.path, true));

        let profile = runtime_profile(&config).await.unwrap();

        profile
            .event_store
            .append(event("persistent-run", 1, AgentEventKind::RunStarted))
            .await
            .unwrap();
        let loaded = profile.event_store.load("persistent-run").await.unwrap();
        assert_eq!(loaded.len(), 1);
    }

    #[tokio::test]
    async fn profile_omitted_event_store_uses_state_database() {
        let db = TempSqliteDb::new("default-profile-event-store");
        let mut config = runtime_profile_config(sqlite_event_store(&db.path, true));
        config.event_store = None;
        let database_url = sqlite_database_url(&db.path);

        let profile = RuntimeProfile::try_from_config(
            &config,
            Some(&models_dev_registry()),
            None,
            Some(&database_url),
        )
        .await
        .unwrap();
        profile
            .event_store
            .append(event("default-event-run", 1, AgentEventKind::RunStarted))
            .await
            .unwrap();
        drop(profile);

        let reloaded_store = SqliteEventStore::connect(SqliteEventStoreConfig::new(database_url))
            .await
            .unwrap();
        let loaded = reloaded_store.load("default-event-run").await.unwrap();

        assert_eq!(loaded.len(), 1);
    }

    #[tokio::test]
    async fn sqlite_event_store_reloads_events_across_profile_rebuilds() {
        let db = TempSqliteDb::new("profile-event-reload");
        let config = runtime_profile_config(sqlite_event_store(&db.path, true));

        let first_profile = runtime_profile(&config).await.unwrap();
        first_profile
            .event_store
            .append(event("reloaded-run", 1, AgentEventKind::RunStarted))
            .await
            .unwrap();
        drop(first_profile);

        let second_profile = runtime_profile(&config).await.unwrap();
        let loaded = second_profile
            .event_store
            .load("reloaded-run")
            .await
            .unwrap();

        assert_eq!(loaded.len(), 1);
        assert!(matches!(loaded[0].kind, AgentEventKind::RunStarted));
    }

    #[tokio::test]
    async fn profile_runtime_writes_events_to_configured_event_store() {
        let db = TempSqliteDb::new("profile-event-runtime");
        let config = runtime_profile_config(sqlite_event_store(&db.path, true));
        let mut profile = runtime_profile(&config).await.unwrap();
        profile.provider = Arc::new(TextModelProvider);
        let session = AgentSession::builder().build();
        let manifest = AgentManifest::default();

        let runtime = profile.build_runtime(&session, &manifest).await.unwrap();
        let report = runtime.run("hello").await.unwrap();
        let reloaded_store =
            SqliteEventStore::connect(SqliteEventStoreConfig::new(sqlite_database_url(&db.path)))
                .await
                .unwrap();
        let loaded = reloaded_store.load(&report.run_id).await.unwrap();

        assert_eq!(loaded, report.events);
    }

    #[tokio::test]
    async fn sqlite_event_store_without_migration_reports_missing_schema() {
        let db = TempSqliteDb::new("profile-event-no-schema");
        let config = runtime_profile_config(sqlite_event_store(&db.path, false));

        let error = match runtime_profile(&config).await {
            Ok(_) => panic!("event store without schema should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("schema is missing"));
    }

    #[tokio::test]
    async fn example_telegram_openrouter_free_profile_builds_registry() {
        let config = serde_json::from_str::<HostProfileConfig>(include_str!(
            "../examples/profile-configs/telegram-openrouter-free.json"
        ))
        .unwrap();

        let registry = build_registry_with_state_database_url(&config, "sqlite::memory:")
            .await
            .unwrap();

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

        let registry = build_registry_with_state_database_url(&config, "sqlite::memory:")
            .await
            .unwrap();

        assert_eq!(
            registry.profile_descriptors()[0].profile_id,
            "chatgpt-codex"
        );
    }

    #[tokio::test]
    async fn example_chatgpt_codex_subagent_smoke_profile_builds_registry() {
        let config = serde_json::from_str::<HostProfileConfig>(include_str!(
            "../examples/profile-configs/chatgpt-codex-subagent-smoke.json"
        ))
        .unwrap();

        let registry = build_registry_with_state_database_url(&config, "sqlite::memory:")
            .await
            .unwrap();

        assert_eq!(
            registry.profile_descriptors()[0].profile_id,
            "chatgpt-codex-subagent-smoke"
        );
    }

    #[tokio::test]
    async fn example_plugin_stdio_profile_builds_registry() {
        let mut config = serde_json::from_str::<HostProfileConfig>(include_str!(
            "../examples/profile-configs/plugin-stdio-example.json"
        ))
        .unwrap();
        let db = TempSqliteDb::new("plugin-stdio-example");
        config.profiles[0].event_store = Some(sqlite_event_store(&db.path, true));

        let registry = build_registry_with_state_database_url(&config, "sqlite::memory:")
            .await
            .unwrap();

        assert_eq!(
            registry.profile_descriptors()[0].profile_id,
            "plugin-stdio-example"
        );
    }

    #[tokio::test]
    async fn profile_default_plugins_become_default_manifest_patches() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "default",
                "displayName": "Default",
                "provider": {"type": "responses", "model": "gpt-5.4-mini"},
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

        let profile = runtime_profile(&config).await.unwrap();
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
        let profile = runtime_profile(&config).await.unwrap();
        let session = AgentSession::builder().build();
        let manifest = AgentManifest::default();

        let runtime = profile.build_runtime(&session, &manifest).await.unwrap();
        let compaction = runtime
            .context_compaction_config()
            .expect("auto ChatGPT profile should register compaction");

        assert_eq!(compaction.input_limit_tokens, 272_000);
        assert_eq!(compaction.trigger_ratio, 0.9);
        assert_eq!(compaction.trigger_threshold(), 244_800);
        assert_eq!(compaction.mode, ContextCompactionMode::PersistentState);
        assert_eq!(compaction.metadata["source"], "openai_responses");
        assert_eq!(compaction.metadata["profileId"], "chatgpt");
        assert_eq!(compaction.metadata["inputLimitModel"], "gpt-5.4-mini");
        assert_eq!(compaction.metadata["compactModel"], "gpt-5.4-mini");
    }

    #[tokio::test]
    async fn chatgpt_compaction_can_lookup_input_limit_model_separately() {
        let config = serde_json::from_str::<RuntimeProfileConfig>(
            r#"{
                "profileId": "chatgpt",
                "displayName": "ChatGPT",
                "provider": {"type": "chatgpt_responses", "model": "gpt-5.4-mini"},
                "compaction": {
                    "type": "openai_responses",
                    "inputLimitModel": "compact-only",
                    "compactModel": "gpt-5.4-mini"
                }
            }"#,
        )
        .unwrap();
        let profile = runtime_profile(&config).await.unwrap();
        let session = AgentSession::builder().build();
        let manifest = AgentManifest::default();

        let runtime = profile.build_runtime(&session, &manifest).await.unwrap();
        let compaction = runtime
            .context_compaction_config()
            .expect("explicit compaction should be registered");

        assert_eq!(compaction.input_limit_tokens, 32_000);
        assert_eq!(compaction.trigger_threshold(), 28_800);
        assert_eq!(compaction.metadata["inputLimitModel"], "compact-only");
        assert_eq!(compaction.metadata["compactModel"], "gpt-5.4-mini");
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
        let profile = runtime_profile(&config).await.unwrap();
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
                    "inputLimitModel": "compact-only",
                    "compactModel": "gpt-5.4-mini",
                    "inputLimitTokens": 200000,
                    "triggerRatio": 0.8,
                    "summaryBudgetTokens": 32000,
                    "keepRecentTokens": 64000,
                    "mode": "request_only",
                    "requestTimeoutSecs": 120
                }
            }"#,
        )
        .unwrap();
        let profile = runtime_profile(&config).await.unwrap();
        let session = AgentSession::builder().build();
        let manifest = AgentManifest::default();

        let runtime = profile.build_runtime(&session, &manifest).await.unwrap();
        let compaction = runtime
            .context_compaction_config()
            .expect("explicit compaction should be registered");

        assert_eq!(compaction.input_limit_tokens, 200_000);
        assert_eq!(compaction.trigger_ratio, 0.8);
        assert_eq!(compaction.trigger_threshold(), 160_000);
        assert_eq!(compaction.summary_budget_tokens, 32_000);
        assert_eq!(compaction.keep_recent_tokens, 64_000);
        assert_eq!(compaction.mode, ContextCompactionMode::RequestOnly);
        assert_eq!(compaction.metadata["inputLimitModel"], "compact-only");
        assert_eq!(compaction.metadata["compactModel"], "gpt-5.4-mini");
    }

    fn runtime_profile_config(event_store: ProfileEventStoreConfig) -> RuntimeProfileConfig {
        RuntimeProfileConfig {
            profile_id: "default".into(),
            display_name: "Default".into(),
            description: None,
            provider: responses_provider(),
            event_store: Some(event_store),
            compaction: Default::default(),
            plugins: Vec::new(),
            manifest_patches: Vec::new(),
            metadata: Default::default(),
        }
    }

    fn responses_provider() -> BuiltInProviderConfig {
        serde_json::from_value(serde_json::json!({
            "type": "responses",
            "model": "gpt-5.4-mini"
        }))
        .unwrap()
    }

    fn sqlite_event_store(path: &Path, migrate_on_connect: bool) -> ProfileEventStoreConfig {
        ProfileEventStoreConfig::Sqlite {
            database_url: sqlite_database_url(path),
            migrate_on_connect,
        }
    }

    fn sqlite_database_url(path: &Path) -> String {
        format!("sqlite:{}", path.display())
    }

    fn event(run_id: &str, sequence: u64, kind: AgentEventKind) -> AgentEvent {
        AgentEvent {
            run_id: run_id.into(),
            sequence,
            turn_id: None,
            phase: None,
            kind,
        }
    }

    struct TempSqliteDb {
        path: PathBuf,
    }

    impl TempSqliteDb {
        fn new(name: &str) -> Self {
            let id = NEXT_DB_ID.fetch_add(1, Ordering::SeqCst);
            Self {
                path: std::env::temp_dir().join(format!(
                    "noloong-profile-event-store-{name}-{}-{id}.sqlite",
                    std::process::id()
                )),
            }
        }
    }

    impl Drop for TempSqliteDb {
        fn drop(&mut self) {
            remove_if_exists(&self.path);
            remove_if_exists(&self.path.with_extension("sqlite-shm"));
            remove_if_exists(&self.path.with_extension("sqlite-wal"));
        }
    }

    fn remove_if_exists(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    struct TextModelProvider;

    impl ModelProvider for TextModelProvider {
        fn id(&self) -> &str {
            "test-model"
        }

        fn model_name(&self) -> Option<&str> {
            Some("gpt-5.4-mini")
        }

        fn stream_model<'a>(
            &'a self,
            _request: ModelRequest,
            sink: ModelStreamSink,
            _cancellation: CancellationToken,
        ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
            Box::pin(async move {
                let events = vec![
                    ModelStreamEvent::TextDelta {
                        text: "hello".into(),
                    },
                    ModelStreamEvent::Finished {
                        stop_reason: StopReason::Stop,
                    },
                ];
                for event in &events {
                    sink(event.clone()).await?;
                }
                Ok(events)
            })
        }
    }
}

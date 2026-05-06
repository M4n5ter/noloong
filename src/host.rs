use crate::config::{
    BuiltInProviderConfig, CliConfigError, EnvAuthProviderConfig, HostProfileConfig,
    RegistryStoreConfig, RuntimeProfileConfig,
};
use noloong_agent::{
    AgentManifest, AgentSession,
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
    HttpAuthContext, HttpAuthHeader, HttpAuthHeaders, HttpAuthProvider, ModelProvider,
    ResponsesApiProvider, ResponsesApiProviderConfig,
};
use opendal::{
    Operator,
    services::{Fs, Memory},
};
use std::{env, sync::Arc};
use thiserror::Error;

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
}

impl RuntimeProfile {
    fn try_from_config(config: &RuntimeProfileConfig) -> Result<Self, HostBuildError> {
        let mut validated_manifest = AgentManifest::default();
        for patch in &config.manifest_patches {
            validated_manifest
                .apply_patch(patch.clone())
                .map_err(|error| {
                    HostBuildError::Config(CliConfigError::ParseConfig(error.to_string()))
                })?;
        }
        Ok(Self {
            descriptor: InteractionProfileDescriptor {
                profile_id: config.profile_id.clone(),
                display_name: config.display_name.clone(),
                description: config.description.clone(),
                default_manifest_patches: config.manifest_patches.clone(),
                metadata: config.metadata.clone(),
            },
            provider: build_provider(&config.profile_id, &config.provider)?,
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
            session
                .runtime_builder()
                .with_model_provider(Arc::clone(&self.provider))
                .build()
                .map_err(InteractionError::from)
        })
    }
}

fn build_provider(
    profile_id: &str,
    config: &BuiltInProviderConfig,
) -> Result<Arc<dyn ModelProvider>, HostBuildError> {
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
            Ok(Arc::new(ChatCompletionsProvider::new(provider)?))
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
            Ok(Arc::new(ResponsesApiProvider::new(provider)?))
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
            Ok(Arc::new(AnthropicMessagesProvider::new(provider)?))
        }
        BuiltInProviderConfig::ChatgptResponses {
            provider_id,
            model,
            auth,
        } => {
            let auth_provider = Arc::new(EnvHttpAuthProvider::from_config(auth.clone()));
            let provider = noloong_openai::provider::chatgpt_responses_provider(
                provider_id.clone().unwrap_or_else(|| profile_id.into()),
                model,
                auth_provider,
            )?;
            Ok(Arc::new(provider))
        }
    }
}

#[derive(Clone)]
struct EnvHttpAuthProvider {
    id: String,
    headers: Vec<EnvAuthHeader>,
}

impl EnvHttpAuthProvider {
    fn from_config(config: EnvAuthProviderConfig) -> Self {
        match config {
            EnvAuthProviderConfig::EnvHeaders { id, headers } => Self {
                id,
                headers: headers
                    .into_iter()
                    .map(|header| EnvAuthHeader {
                        name: header.name,
                        env: header.env,
                        value_prefix: header.value_prefix,
                    })
                    .collect(),
            },
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
    use super::build_registry;
    use crate::config::HostProfileConfig;

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
}

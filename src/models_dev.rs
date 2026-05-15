use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
    time::Duration,
};

const DEFAULT_MODELS_DEV_URL: &str = "https://models.dev";
const MODELS_DEV_URL_ENV: &str = "NOLOONG_MODELS_DEV_URL";
const MODELS_DEV_CACHE_ENV: &str = "NOLOONG_MODELS_DEV_CACHE";
const MODELS_DEV_DISABLE_REFRESH_ENV: &str = "NOLOONG_MODELS_DEV_DISABLE_REFRESH";
const SNAPSHOT: &str = include_str!("models_dev_snapshot.json");

#[derive(Clone, Debug)]
pub struct ModelsDevRegistry {
    providers: ModelsDevProviders,
    cache_path: PathBuf,
    source_url: String,
    refresh_enabled: bool,
}

impl ModelsDevRegistry {
    pub async fn load_default() -> Self {
        let cache_path = default_cache_path();
        let source_url = env::var(MODELS_DEV_URL_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODELS_DEV_URL.into());
        let refresh_enabled = !env_bool(MODELS_DEV_DISABLE_REFRESH_ENV);
        Self::load(cache_path, source_url, refresh_enabled).await
    }

    async fn load(cache_path: PathBuf, source_url: String, refresh_enabled: bool) -> Self {
        let providers = load_cache(&cache_path)
            .await
            .or_else(|| parse_providers(SNAPSHOT).ok())
            .unwrap_or_default();
        Self {
            providers,
            cache_path,
            source_url,
            refresh_enabled,
        }
    }

    pub fn input_limit(&self, provider_id: &str, model_id: &str) -> Option<u64> {
        self.providers
            .get(provider_id)
            .and_then(|provider| provider.models.get(model_id))
            .map(|model| model.limit.input.unwrap_or(model.limit.context))
    }

    pub fn refresh_cache_in_background(&self) {
        if !self.refresh_enabled {
            return;
        }
        let cache_path = self.cache_path.clone();
        let source_url = self.source_url.clone();
        tokio::spawn(async move {
            if let Err(error) = refresh_cache(&source_url, &cache_path).await {
                log::warn!("failed to refresh Models.dev registry: {error}");
            }
        });
    }

    #[cfg(test)]
    pub fn from_json_for_tests(text: &str) -> Self {
        Self {
            providers: parse_providers(text).expect("test Models.dev registry JSON parses"),
            cache_path: PathBuf::new(),
            source_url: String::new(),
            refresh_enabled: false,
        }
    }
}

async fn load_cache(path: &Path) -> Option<ModelsDevProviders> {
    let text = tokio::fs::read_to_string(path).await.ok()?;
    parse_providers(&text).ok()
}

async fn refresh_cache(source_url: &str, cache_path: &Path) -> Result<(), ModelsDevError> {
    let source_url = source_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let text = client
        .get(format!("{source_url}/api.json"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    parse_providers(&text)?;
    if let Some(parent) = cache_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(cache_path, text).await?;
    Ok(())
}

fn parse_providers(text: &str) -> Result<ModelsDevProviders, serde_json::Error> {
    serde_json::from_str(text)
}

fn default_cache_path() -> PathBuf {
    if let Ok(path) = env::var(MODELS_DEV_CACHE_ENV)
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache")
        .join("noloong")
        .join("models-dev.json")
}

fn env_bool(name: &str) -> bool {
    env::var(name)
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

#[derive(Debug, thiserror::Error)]
enum ModelsDevError {
    #[error("http failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("json failed: {0}")]
    Json(#[from] serde_json::Error),
}

type ModelsDevProviders = BTreeMap<String, ModelsDevProvider>;

#[derive(Clone, Debug, Default, Deserialize)]
struct ModelsDevProvider {
    #[serde(default)]
    models: BTreeMap<String, ModelsDevModel>,
}

#[derive(Clone, Debug, Deserialize)]
struct ModelsDevModel {
    limit: ModelsDevLimit,
}

#[derive(Clone, Debug, Deserialize)]
struct ModelsDevLimit {
    context: u64,
    #[serde(default)]
    input: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::ModelsDevRegistry;
    use crate::test_support::{remove_temp_file, write_temp_file};

    #[test]
    fn snapshot_resolves_openai_model_input_limit() {
        let registry = ModelsDevRegistry::from_json_for_tests(super::SNAPSHOT);

        assert_eq!(
            registry.input_limit("openai", "gpt-5.4-mini"),
            Some(272_000)
        );
    }

    #[test]
    fn input_limit_falls_back_to_context_limit() {
        let registry = ModelsDevRegistry::from_json_for_tests(
            r#"{
                "acme": {
                    "models": {
                        "acme-1": {
                            "limit": {"context": 128000, "output": 8192}
                        }
                    }
                }
            }"#,
        );

        assert_eq!(registry.input_limit("acme", "acme-1"), Some(128_000));
    }

    #[tokio::test]
    async fn cache_loads_valid_registry_and_rejects_invalid_json() {
        let path = write_temp_file(
            "models-dev-cache",
            "json",
            r#"{
                "openai": {
                    "models": {
                        "test-model": {
                            "limit": {"context": 1000, "input": 900, "output": 100}
                        }
                    }
                }
            }"#,
        );

        let providers = super::load_cache(&path).await.expect("cache should parse");
        assert_eq!(
            providers
                .get("openai")
                .and_then(|provider| provider.models.get("test-model"))
                .map(|model| model.limit.input.unwrap_or(model.limit.context)),
            Some(900)
        );

        tokio::fs::write(&path, "{not json").await.unwrap();
        assert!(super::load_cache(&path).await.is_none());
        remove_temp_file(path);
    }

    #[tokio::test]
    async fn load_prefers_cache_over_snapshot_and_falls_back_when_cache_is_invalid() {
        let path = write_temp_file(
            "models-dev-load",
            "json",
            r#"{
                "openai": {
                    "models": {
                        "gpt-5.4-mini": {
                            "limit": {"context": 1000, "input": 900, "output": 100}
                        }
                    }
                }
            }"#,
        );

        let registry = ModelsDevRegistry::load(path.clone(), String::new(), false).await;
        assert_eq!(registry.input_limit("openai", "gpt-5.4-mini"), Some(900));

        tokio::fs::write(&path, "{not json").await.unwrap();
        let registry = ModelsDevRegistry::load(path.clone(), String::new(), false).await;
        assert_eq!(
            registry.input_limit("openai", "gpt-5.4-mini"),
            Some(272_000)
        );
        remove_temp_file(path);
    }
}

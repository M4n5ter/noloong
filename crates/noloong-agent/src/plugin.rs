use noloong_agent_core::{ExtensionCapabilitySelector, StdioExtensionConfig};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Duration,
};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentPluginDeclaration {
    pub plugin_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub transport: PluginTransport,
    #[serde(default)]
    pub allowed_capabilities: Vec<ExtensionCapabilitySelector>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub on_load_failure: PluginLoadFailurePolicy,
}

impl AgentPluginDeclaration {
    pub fn validate(&self) -> Result<(), PluginDeclarationError> {
        validate_non_empty("pluginId", &self.plugin_id)?;
        validate_non_empty("displayName", &self.display_name)?;
        self.transport.validate()?;
        validate_capability_selectors(&self.allowed_capabilities)?;
        Ok(())
    }

    pub fn summary(&self) -> String {
        let capabilities = self
            .allowed_capabilities
            .iter()
            .map(capability_selector_summary)
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "plugin {} ({}) transport={} enabled={} onLoadFailure={} allowedCapabilities=[{}]",
            self.plugin_id,
            self.display_name,
            self.transport.summary(),
            self.enabled,
            self.on_load_failure.as_str(),
            capabilities,
        )
    }

    pub fn to_stdio_extension_config(
        &self,
        env_source: impl Fn(&str) -> Option<String>,
    ) -> Result<StdioExtensionConfig, PluginLoadError> {
        self.validate()
            .map_err(|error| PluginLoadError::InvalidDeclaration {
                plugin_id: self.plugin_id.clone(),
                message: error.to_string(),
            })?;
        match &self.transport {
            PluginTransport::Stdio(transport) => {
                let mut config = StdioExtensionConfig::new(&transport.command)
                    .args(transport.args.clone())
                    .clear_env(true)
                    .allowed_capabilities(self.allowed_capabilities.iter().cloned().collect());
                if let Some(cwd) = &transport.cwd {
                    config = config.cwd(cwd.clone());
                }
                for (target_name, source) in &transport.env {
                    let value = source.resolve(&self.plugin_id, target_name, &env_source)?;
                    config = config.env(target_name.clone(), value);
                }
                if let Some(request_timeout_secs) = transport.request_timeout_secs {
                    config = config.request_timeout(Duration::from_secs(request_timeout_secs));
                }
                if let Some(stream_timeout_secs) = transport.stream_timeout_secs {
                    config = config.stream_timeout(Duration::from_secs(stream_timeout_secs));
                }
                Ok(config)
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PluginTransport {
    Stdio(StdioPluginTransport),
}

impl PluginTransport {
    pub fn validate(&self) -> Result<(), PluginDeclarationError> {
        match self {
            Self::Stdio(transport) => transport.validate(),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Stdio(transport) => transport.summary(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StdioPluginTransport {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, PluginEnvSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_timeout_secs: Option<u64>,
}

impl StdioPluginTransport {
    pub fn validate(&self) -> Result<(), PluginDeclarationError> {
        validate_non_empty("command", &self.command)?;
        for arg in &self.args {
            if arg.contains('\0') {
                return Err(PluginDeclarationError::Invalid(
                    "plugin args must not contain NUL bytes".into(),
                ));
            }
        }
        for (target_name, source) in &self.env {
            validate_non_empty("env target name", target_name)?;
            source.validate()?;
        }
        validate_positive_timeout("requestTimeoutSecs", self.request_timeout_secs)?;
        validate_positive_timeout("streamTimeoutSecs", self.stream_timeout_secs)?;
        Ok(())
    }

    pub fn summary(&self) -> String {
        let args = self.args.join(" ");
        let cwd = self
            .cwd
            .as_ref()
            .map(|cwd| cwd.display().to_string())
            .unwrap_or_else(|| "(session cwd)".into());
        let env = self
            .env
            .iter()
            .map(|(target, source)| format!("{target}<={}", source.summary()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "stdio command=`{}` args=`{}` cwd=`{}` env=[{}]",
            self.command, args, cwd, env
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PluginEnvSource {
    HostEnv { name: String },
}

impl PluginEnvSource {
    pub fn validate(&self) -> Result<(), PluginDeclarationError> {
        match self {
            Self::HostEnv { name } => validate_non_empty("host env name", name),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::HostEnv { name } => format!("host_env:{name}"),
        }
    }

    fn resolve(
        &self,
        plugin_id: &str,
        target_name: &str,
        env_source: &impl Fn(&str) -> Option<String>,
    ) -> Result<String, PluginLoadError> {
        match self {
            Self::HostEnv { name } => env_source(name)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| PluginLoadError::MissingHostEnv {
                    plugin_id: plugin_id.into(),
                    target_name: target_name.into(),
                    source_name: name.clone(),
                }),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginLoadFailurePolicy {
    #[default]
    DisableForRun,
    FailRun,
}

impl PluginLoadFailurePolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DisableForRun => "disable_for_run",
            Self::FailRun => "fail_run",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginDeclarationError {
    Invalid(String),
}

impl std::fmt::Display for PluginDeclarationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for PluginDeclarationError {}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginLoadWarning {
    pub plugin_id: String,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum PluginLoadError {
    #[error("invalid plugin declaration `{plugin_id}`: {message}")]
    InvalidDeclaration { plugin_id: String, message: String },
    #[error(
        "plugin `{plugin_id}` requires host environment variable `{source_name}` for child env `{target_name}`"
    )]
    MissingHostEnv {
        plugin_id: String,
        target_name: String,
        source_name: String,
    },
    #[error("plugin `{plugin_id}` failed to load: {message}")]
    Startup { plugin_id: String, message: String },
}

fn validate_non_empty(field: &str, value: &str) -> Result<(), PluginDeclarationError> {
    if value.trim().is_empty() {
        return Err(PluginDeclarationError::Invalid(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

fn validate_positive_timeout(
    field: &str,
    value: Option<u64>,
) -> Result<(), PluginDeclarationError> {
    if value == Some(0) {
        return Err(PluginDeclarationError::Invalid(format!(
            "{field} must be greater than zero"
        )));
    }
    Ok(())
}

fn validate_capability_selectors(
    selectors: &[ExtensionCapabilitySelector],
) -> Result<(), PluginDeclarationError> {
    let mut seen = BTreeSet::new();
    for selector in selectors {
        selector
            .validate()
            .map_err(PluginDeclarationError::Invalid)?;
        if !seen.insert(selector) {
            return Err(PluginDeclarationError::Invalid(format!(
                "duplicate allowed capability: {}",
                capability_selector_summary(selector)
            )));
        }
    }
    Ok(())
}

fn capability_selector_summary(selector: &ExtensionCapabilitySelector) -> String {
    match selector {
        ExtensionCapabilitySelector::ModelProvider { id } => format!("model_provider:{id}"),
        ExtensionCapabilitySelector::Tool { name } => format!("tool:{name}"),
        ExtensionCapabilitySelector::ContextProvider { id } => format!("context_provider:{id}"),
        ExtensionCapabilitySelector::PhaseNode { id } => format!("phase_node:{id}"),
        ExtensionCapabilitySelector::PhaseHook { id } => format!("phase_hook:{id}"),
        ExtensionCapabilitySelector::ToolCallHook { id } => format!("tool_call_hook:{id}"),
        ExtensionCapabilitySelector::CompactionSummarizer { id } => {
            format!("compaction_summarizer:{id}")
        }
        ExtensionCapabilitySelector::ContextCompactor { id } => {
            format!("context_compactor:{id}")
        }
        ExtensionCapabilitySelector::HttpAuthProvider { id } => format!("http_auth_provider:{id}"),
    }
}

use crate::ExtensionCapabilitySelector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentPluginDeclaration {
    pub plugin_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub components: Vec<PluginComponent>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub on_load_failure: PluginLoadFailurePolicy,
}

impl AgentPluginDeclaration {
    pub fn validate(&self) -> Result<(), PluginDeclarationError> {
        validate_non_empty("pluginId", &self.plugin_id)?;
        validate_non_empty("displayName", &self.display_name)?;
        if self.components.is_empty() {
            return Err(PluginDeclarationError::Invalid(
                "components must contain at least one component".into(),
            ));
        }
        for component in &self.components {
            component.validate()?;
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let components = self
            .components
            .iter()
            .map(PluginComponent::summary)
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "plugin {} ({}) enabled={} onLoadFailure={} components=[{}]",
            self.plugin_id,
            self.display_name,
            self.enabled,
            self.on_load_failure.as_str(),
            components,
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PluginComponent {
    Skills(SkillsPluginComponent),
    Mcp(McpPluginComponent),
    NoloongExtension(NoloongExtensionPluginComponent),
}

impl PluginComponent {
    pub fn validate(&self) -> Result<(), PluginDeclarationError> {
        match self {
            Self::Skills(component) => component.validate(),
            Self::Mcp(component) => component.validate(),
            Self::NoloongExtension(component) => component.validate(),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Skills(component) => component.summary(),
            Self::Mcp(component) => component.summary(),
            Self::NoloongExtension(component) => component.summary(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillsPluginComponent {
    pub roots: Vec<PathBuf>,
}

impl SkillsPluginComponent {
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        if self.roots.is_empty() {
            return Err(PluginDeclarationError::Invalid(
                "skills roots must contain at least one path".into(),
            ));
        }
        for root in &self.roots {
            validate_non_empty("skills root", &root.display().to_string())?;
        }
        Ok(())
    }

    fn summary(&self) -> String {
        let roots = self
            .roots
            .iter()
            .map(|root| root.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!("skills roots=[{roots}]")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoloongExtensionPluginComponent {
    pub transport: NoloongExtensionTransport,
    #[serde(default)]
    pub allowed_capabilities: Vec<ExtensionCapabilitySelector>,
}

impl NoloongExtensionPluginComponent {
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        self.transport.validate()?;
        validate_capability_selectors(&self.allowed_capabilities)
    }

    fn summary(&self) -> String {
        let capabilities = self
            .allowed_capabilities
            .iter()
            .map(capability_selector_summary)
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "noloong_extension transport={} allowedCapabilities=[{}]",
            self.transport.summary(),
            capabilities,
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum NoloongExtensionTransport {
    Stdio(StdioPluginTransport),
}

impl NoloongExtensionTransport {
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        match self {
            Self::Stdio(transport) => transport.validate(),
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::Stdio(transport) => transport.summary(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpPluginComponent {
    pub server_id: String,
    pub transport: McpPluginTransport,
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_timeout_secs: Option<u64>,
}

impl McpPluginComponent {
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        validate_non_empty("serverId", &self.server_id)?;
        self.transport.validate()?;
        validate_string_list("enabledTools", &self.enabled_tools)?;
        validate_string_list("disabledTools", &self.disabled_tools)?;
        if let Some(prefix) = &self.tool_name_prefix {
            validate_non_empty("toolNamePrefix", prefix)?;
        }
        validate_positive_timeout("requestTimeoutSecs", self.request_timeout_secs)
    }

    fn summary(&self) -> String {
        let prefix = self.tool_name_prefix.as_deref().unwrap_or("(default)");
        format!(
            "mcp serverId={} transport={} toolNamePrefix={} enabledTools={} disabledTools={}",
            self.server_id,
            self.transport.summary(),
            prefix,
            self.enabled_tools.len(),
            self.disabled_tools.len(),
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum McpPluginTransport {
    Stdio(McpStdioTransport),
    StreamableHttp(McpStreamableHttpTransport),
}

impl McpPluginTransport {
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        match self {
            Self::Stdio(transport) => transport.validate(),
            Self::StreamableHttp(transport) => transport.validate(),
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::Stdio(transport) => transport.summary(),
            Self::StreamableHttp(transport) => transport.summary(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpStdioTransport {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, PluginEnvSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_timeout_secs: Option<u64>,
}

impl McpStdioTransport {
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        validate_stdio_command(&self.command, &self.args)?;
        for (target_name, source) in &self.env {
            validate_non_empty("env target name", target_name)?;
            source.validate()?;
        }
        validate_positive_timeout("requestTimeoutSecs", self.request_timeout_secs)
    }

    fn summary(&self) -> String {
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpStreamableHttpTransport {
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, McpHeaderSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_timeout_secs: Option<u64>,
}

impl McpStreamableHttpTransport {
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        validate_non_empty("url", &self.url)?;
        for (name, source) in &self.headers {
            validate_non_empty("header name", name)?;
            source.validate()?;
        }
        validate_positive_timeout("connectTimeoutSecs", self.connect_timeout_secs)?;
        validate_positive_timeout("requestTimeoutSecs", self.request_timeout_secs)
    }

    fn summary(&self) -> String {
        let headers = self
            .headers
            .iter()
            .map(|(name, source)| format!("{name}<={}", source.summary()))
            .collect::<Vec<_>>()
            .join(", ");
        format!("streamable_http url={} headers=[{}]", self.url, headers)
    }
}

#[derive(Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum McpHeaderSource {
    Static {
        value: String,
    },
    HostEnv {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
    },
}

impl fmt::Debug for McpHeaderSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static { .. } => formatter
                .debug_struct("Static")
                .field("value", &"<redacted>")
                .finish(),
            Self::HostEnv { name, prefix } => formatter
                .debug_struct("HostEnv")
                .field("name", name)
                .field("prefix", &prefix.as_ref().map(|_| "<redacted>"))
                .finish(),
        }
    }
}

impl McpHeaderSource {
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        match self {
            Self::Static { value } => validate_non_empty("static header value", value),
            Self::HostEnv { name, .. } => validate_non_empty("host env name", name),
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::Static { .. } => "static:<redacted>".into(),
            Self::HostEnv { name, .. } => format!("host_env:{name}"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
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
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        validate_stdio_command(&self.command, &self.args)?;
        for (target_name, source) in &self.env {
            validate_non_empty("env target name", target_name)?;
            source.validate()?;
        }
        validate_positive_timeout("requestTimeoutSecs", self.request_timeout_secs)?;
        validate_positive_timeout("streamTimeoutSecs", self.stream_timeout_secs)
    }

    fn summary(&self) -> String {
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

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PluginEnvSource {
    HostEnv { name: String },
}

impl PluginEnvSource {
    fn validate(&self) -> Result<(), PluginDeclarationError> {
        match self {
            Self::HostEnv { name } => validate_non_empty("host env name", name),
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::HostEnv { name } => format!("host_env:{name}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
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

fn validate_stdio_command(command: &str, args: &[String]) -> Result<(), PluginDeclarationError> {
    validate_non_empty("command", command)?;
    for arg in args {
        if arg.contains('\0') {
            return Err(PluginDeclarationError::Invalid(
                "plugin args must not contain NUL bytes".into(),
            ));
        }
    }
    Ok(())
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

fn validate_string_list(field: &str, values: &[String]) -> Result<(), PluginDeclarationError> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_non_empty(field, value)?;
        if !seen.insert(value) {
            return Err(PluginDeclarationError::Invalid(format!(
                "duplicate {field} entry: {value}"
            )));
        }
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

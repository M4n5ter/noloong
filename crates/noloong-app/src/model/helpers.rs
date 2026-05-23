use super::types::{PluginOverview, ReasoningSummary};
use noloong_config::{
    AgentPluginDeclaration, BuiltInProviderConfig, ContextCompactionMode,
    ExtensionCapabilitySelector, McpPluginTransport, NoloongExtensionTransport,
    RegistryStoreConfig, ResponsesProviderReasoningConfig, ResponsesProviderReasoningEffort,
    ResponsesProviderReasoningSummary, StdioPluginTransport,
};

pub(super) fn optional_string(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

pub(super) fn optional_u64(value: String) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        value.parse().ok()
    }
}

pub(super) fn optional_f64(value: String) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        value.parse().ok()
    }
}

pub(super) fn sanitize_profile_id(value: &str) -> String {
    let id: String = value
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if id.is_empty() { "provider".into() } else { id }
}

pub(super) fn split_lines(value: String) -> Vec<String> {
    value
        .lines()
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn provider_id(provider: &BuiltInProviderConfig) -> Option<String> {
    match provider {
        BuiltInProviderConfig::ChatCompletions { provider_id, .. }
        | BuiltInProviderConfig::Responses { provider_id, .. }
        | BuiltInProviderConfig::AnthropicMessages { provider_id, .. }
        | BuiltInProviderConfig::ChatgptResponses { provider_id, .. } => provider_id.clone(),
    }
}

pub(super) fn provider_id_mut(provider: &mut BuiltInProviderConfig) -> &mut Option<String> {
    match provider {
        BuiltInProviderConfig::ChatCompletions { provider_id, .. }
        | BuiltInProviderConfig::Responses { provider_id, .. }
        | BuiltInProviderConfig::AnthropicMessages { provider_id, .. }
        | BuiltInProviderConfig::ChatgptResponses { provider_id, .. } => provider_id,
    }
}

pub(super) fn base_url(provider: &BuiltInProviderConfig) -> Option<String> {
    match provider {
        BuiltInProviderConfig::ChatCompletions { base_url, .. }
        | BuiltInProviderConfig::Responses { base_url, .. }
        | BuiltInProviderConfig::AnthropicMessages { base_url, .. } => base_url.clone(),
        BuiltInProviderConfig::ChatgptResponses { .. } => None,
    }
}

pub(super) fn base_url_supported(provider: &BuiltInProviderConfig) -> bool {
    !matches!(provider, BuiltInProviderConfig::ChatgptResponses { .. })
}

pub(super) fn base_url_mut(provider: &mut BuiltInProviderConfig) -> Option<&mut Option<String>> {
    match provider {
        BuiltInProviderConfig::ChatCompletions { base_url, .. }
        | BuiltInProviderConfig::Responses { base_url, .. }
        | BuiltInProviderConfig::AnthropicMessages { base_url, .. } => Some(base_url),
        BuiltInProviderConfig::ChatgptResponses { .. } => None,
    }
}

pub(super) fn api_key_env(provider: &BuiltInProviderConfig) -> Option<String> {
    match provider {
        BuiltInProviderConfig::ChatCompletions { api_key_env, .. }
        | BuiltInProviderConfig::Responses { api_key_env, .. }
        | BuiltInProviderConfig::AnthropicMessages { api_key_env, .. } => api_key_env.clone(),
        BuiltInProviderConfig::ChatgptResponses { .. } => None,
    }
}

pub(super) fn api_key_env_supported(provider: &BuiltInProviderConfig) -> bool {
    !matches!(provider, BuiltInProviderConfig::ChatgptResponses { .. })
}

pub(super) fn api_key_env_mut(provider: &mut BuiltInProviderConfig) -> Option<&mut Option<String>> {
    match provider {
        BuiltInProviderConfig::ChatCompletions { api_key_env, .. }
        | BuiltInProviderConfig::Responses { api_key_env, .. }
        | BuiltInProviderConfig::AnthropicMessages { api_key_env, .. } => Some(api_key_env),
        BuiltInProviderConfig::ChatgptResponses { .. } => None,
    }
}

pub(super) fn state_mode(provider: &BuiltInProviderConfig) -> Option<&'static str> {
    match provider {
        BuiltInProviderConfig::Responses { state_mode, .. }
        | BuiltInProviderConfig::ChatgptResponses { state_mode, .. } => {
            Some(if state_mode.is_stateless() {
                "stateless"
            } else {
                "stateful"
            })
        }
        _ => None,
    }
}

pub(super) fn allow_file_data_url_input(provider: &BuiltInProviderConfig) -> Option<bool> {
    match provider {
        BuiltInProviderConfig::Responses {
            allow_file_data_url_input,
            ..
        }
        | BuiltInProviderConfig::ChatgptResponses {
            allow_file_data_url_input,
            ..
        } => Some(*allow_file_data_url_input),
        _ => None,
    }
}

pub(super) fn allow_file_data_url_input_mut(
    provider: &mut BuiltInProviderConfig,
) -> Option<&mut bool> {
    match provider {
        BuiltInProviderConfig::Responses {
            allow_file_data_url_input,
            ..
        }
        | BuiltInProviderConfig::ChatgptResponses {
            allow_file_data_url_input,
            ..
        } => Some(allow_file_data_url_input),
        _ => None,
    }
}

pub(super) fn max_tokens(provider: &BuiltInProviderConfig) -> Option<u64> {
    match provider {
        BuiltInProviderConfig::ChatCompletions {
            max_completion_tokens,
            ..
        } => *max_completion_tokens,
        BuiltInProviderConfig::Responses {
            max_output_tokens, ..
        } => *max_output_tokens,
        BuiltInProviderConfig::AnthropicMessages { max_tokens, .. } => *max_tokens,
        BuiltInProviderConfig::ChatgptResponses { .. } => None,
    }
}

pub(super) fn max_tokens_supported(provider: &BuiltInProviderConfig) -> bool {
    !matches!(provider, BuiltInProviderConfig::ChatgptResponses { .. })
}

pub(super) fn max_tokens_mut(provider: &mut BuiltInProviderConfig) -> Option<&mut Option<u64>> {
    match provider {
        BuiltInProviderConfig::ChatCompletions {
            max_completion_tokens,
            ..
        } => Some(max_completion_tokens),
        BuiltInProviderConfig::Responses {
            max_output_tokens, ..
        } => Some(max_output_tokens),
        BuiltInProviderConfig::AnthropicMessages { max_tokens, .. } => Some(max_tokens),
        BuiltInProviderConfig::ChatgptResponses { .. } => None,
    }
}

pub(super) fn responses_reasoning_mut(
    provider: &mut BuiltInProviderConfig,
) -> Option<&mut ResponsesProviderReasoningConfig> {
    match provider {
        BuiltInProviderConfig::Responses { reasoning, .. }
        | BuiltInProviderConfig::ChatgptResponses { reasoning, .. } => reasoning.as_mut(),
        _ => None,
    }
}

pub(super) fn reasoning_summary(provider: &BuiltInProviderConfig) -> Option<ReasoningSummary> {
    match provider {
        BuiltInProviderConfig::ChatCompletions { reasoning, .. } => {
            reasoning.as_ref().map(|reasoning| ReasoningSummary {
                enabled: reasoning.enabled,
                effort: reasoning
                    .effort
                    .map(|effort| effort.as_str().to_string())
                    .unwrap_or_else(|| "default".into()),
                summary: "-".into(),
            })
        }
        BuiltInProviderConfig::Responses { reasoning, .. }
        | BuiltInProviderConfig::ChatgptResponses { reasoning, .. } => {
            reasoning.as_ref().map(responses_reasoning_summary)
        }
        BuiltInProviderConfig::AnthropicMessages { reasoning, .. } => {
            reasoning.as_ref().map(|reasoning| ReasoningSummary {
                enabled: reasoning.thinking.is_none_or(|thinking| {
                    !matches!(
                        thinking,
                        noloong_config::AnthropicProviderThinkingMode::Disabled
                    )
                }),
                effort: reasoning
                    .effort
                    .map(|effort| format!("{effort:?}").to_ascii_lowercase())
                    .unwrap_or_else(|| "default".into()),
                summary: reasoning
                    .thinking
                    .map(|thinking| format!("{thinking:?}").to_ascii_lowercase())
                    .unwrap_or_else(|| "-".into()),
            })
        }
    }
}

pub(super) fn responses_reasoning_summary(
    reasoning: &ResponsesProviderReasoningConfig,
) -> ReasoningSummary {
    ReasoningSummary {
        enabled: reasoning.enabled,
        effort: reasoning
            .effort
            .map(responses_reasoning_effort_as_str)
            .unwrap_or_else(|| "default".into()),
        summary: reasoning
            .summary
            .map(responses_reasoning_summary_as_str)
            .unwrap_or_else(|| "-".into()),
    }
}

pub(super) fn parse_responses_reasoning_effort(
    value: &str,
) -> Option<ResponsesProviderReasoningEffort> {
    match value {
        "minimal" => Some(ResponsesProviderReasoningEffort::Minimal),
        "low" => Some(ResponsesProviderReasoningEffort::Low),
        "medium" => Some(ResponsesProviderReasoningEffort::Medium),
        "high" => Some(ResponsesProviderReasoningEffort::High),
        "xhigh" => Some(ResponsesProviderReasoningEffort::XHigh),
        _ => None,
    }
}

pub(super) fn responses_reasoning_effort_as_str(
    effort: ResponsesProviderReasoningEffort,
) -> String {
    match effort {
        ResponsesProviderReasoningEffort::Minimal => "minimal",
        ResponsesProviderReasoningEffort::Low => "low",
        ResponsesProviderReasoningEffort::Medium => "medium",
        ResponsesProviderReasoningEffort::High => "high",
        ResponsesProviderReasoningEffort::XHigh => "xhigh",
    }
    .into()
}

pub(super) fn parse_responses_reasoning_summary(
    value: &str,
) -> Option<ResponsesProviderReasoningSummary> {
    match value {
        "auto" => Some(ResponsesProviderReasoningSummary::Auto),
        "concise" => Some(ResponsesProviderReasoningSummary::Concise),
        "detailed" => Some(ResponsesProviderReasoningSummary::Detailed),
        "none" => Some(ResponsesProviderReasoningSummary::None),
        _ => None,
    }
}

pub(super) fn responses_reasoning_summary_as_str(
    summary: ResponsesProviderReasoningSummary,
) -> String {
    match summary {
        ResponsesProviderReasoningSummary::Auto => "auto",
        ResponsesProviderReasoningSummary::Concise => "concise",
        ResponsesProviderReasoningSummary::Detailed => "detailed",
        ResponsesProviderReasoningSummary::None => "none",
    }
    .into()
}

pub(super) fn parse_context_compaction_mode(value: &str) -> Option<ContextCompactionMode> {
    match value {
        "persistent_state" => Some(ContextCompactionMode::PersistentState),
        "request_only" => Some(ContextCompactionMode::RequestOnly),
        _ => None,
    }
}

pub(super) fn context_compaction_mode_as_str(mode: ContextCompactionMode) -> &'static str {
    match mode {
        ContextCompactionMode::PersistentState => "persistent_state",
        ContextCompactionMode::RequestOnly => "request_only",
    }
}

pub(super) fn registry_store_summary(store: &RegistryStoreConfig) -> String {
    match store {
        RegistryStoreConfig::Memory => "memory".into(),
        RegistryStoreConfig::Sqlite { database_url } => format!("sqlite: {database_url}"),
        RegistryStoreConfig::Postgres { database_url } => format!("postgres: {database_url}"),
        RegistryStoreConfig::ObjectMemory { prefix } => format!("object-memory: {prefix}"),
        RegistryStoreConfig::ObjectFs { root, prefix } => format!("object-fs: {root} ({prefix})"),
    }
}

pub(super) fn plugin_overview(plugin: &AgentPluginDeclaration) -> PluginOverview {
    PluginOverview {
        plugin_id: plugin.plugin_id.clone(),
        display_name: plugin.display_name.clone(),
        enabled: plugin.enabled,
        component_count: plugin.components.len(),
        on_load_failure: plugin.on_load_failure.as_str().into(),
    }
}

pub(super) fn mcp_transport_kind(transport: &McpPluginTransport) -> &'static str {
    match transport {
        McpPluginTransport::Stdio(_) => "stdio",
        McpPluginTransport::StreamableHttp(_) => "streamable_http",
    }
}

pub(super) fn mcp_transport_endpoint(transport: &McpPluginTransport) -> String {
    match transport {
        McpPluginTransport::Stdio(transport) => stdio_endpoint(&StdioPluginTransport {
            command: transport.command.clone(),
            args: transport.args.clone(),
            cwd: transport.cwd.clone(),
            env: transport.env.clone(),
            request_timeout_secs: transport.request_timeout_secs,
            stream_timeout_secs: None,
        }),
        McpPluginTransport::StreamableHttp(transport) => transport.url.clone(),
    }
}

pub(super) fn mcp_transport_cwd(transport: &McpPluginTransport) -> String {
    match transport {
        McpPluginTransport::Stdio(transport) => transport
            .cwd
            .as_ref()
            .map(|cwd| cwd.display().to_string())
            .unwrap_or_else(|| "session cwd".into()),
        McpPluginTransport::StreamableHttp(_) => "-".into(),
    }
}

pub(super) fn stdio_endpoint(transport: &StdioPluginTransport) -> String {
    let args = transport.args.join(" ");
    if args.is_empty() {
        transport.command.clone()
    } else {
        format!("{} {args}", transport.command)
    }
}

pub(super) fn mcp_transport_env_count(transport: &McpPluginTransport) -> usize {
    match transport {
        McpPluginTransport::Stdio(transport) => transport.env.len(),
        McpPluginTransport::StreamableHttp(_) => 0,
    }
}

pub(super) fn mcp_transport_header_count(transport: &McpPluginTransport) -> usize {
    match transport {
        McpPluginTransport::Stdio(_) => 0,
        McpPluginTransport::StreamableHttp(transport) => transport.headers.len(),
    }
}

pub(super) fn extension_transport_summary(transport: &NoloongExtensionTransport) -> String {
    match transport {
        NoloongExtensionTransport::Stdio(transport) => stdio_endpoint(transport),
    }
}

pub(super) fn capability_selector_summary(selector: &ExtensionCapabilitySelector) -> String {
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
        ExtensionCapabilitySelector::ContextCompactor { id } => format!("context_compactor:{id}"),
        ExtensionCapabilitySelector::HttpAuthProvider { id } => format!("http_auth_provider:{id}"),
    }
}

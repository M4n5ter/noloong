#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReasoningSummary {
    pub enabled: bool,
    pub effort: String,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileProviderSummary {
    pub profile_id: String,
    pub display_name: String,
    pub provider_type: String,
    pub model: String,
    pub is_active: bool,
    pub is_selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompactionEdit {
    pub mode: String,
    pub id: String,
    pub input_limit_model: String,
    pub compact_model: String,
    pub input_limit_tokens: String,
    pub trigger_ratio: String,
    pub summary_budget_tokens: String,
    pub keep_recent_tokens: String,
    pub state_mode: String,
    pub request_timeout_secs: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillRootSummary {
    pub plugin_id: String,
    pub plugin_name: String,
    pub enabled: bool,
    pub root: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillRootEdit {
    pub plugin_id: String,
    pub plugin_name: String,
    pub enabled: bool,
    pub root: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServerSummary {
    pub plugin_id: String,
    pub plugin_name: String,
    pub enabled: bool,
    pub server_id: String,
    pub transport: String,
    pub endpoint: String,
    pub cwd: String,
    pub enabled_tools: usize,
    pub disabled_tools: usize,
    pub tool_name_prefix: String,
    pub request_timeout_secs: Option<u64>,
    pub environment_count: usize,
    pub header_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServerEdit {
    pub server_id: String,
    pub transport: String,
    pub endpoint: String,
    pub args: String,
    pub tool_name_prefix: String,
    pub enabled_tools: String,
    pub disabled_tools: String,
    pub request_timeout_secs: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionSummary {
    pub plugin_id: String,
    pub plugin_name: String,
    pub enabled: bool,
    pub transport: String,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginOverview {
    pub plugin_id: String,
    pub display_name: String,
    pub enabled: bool,
    pub component_count: usize,
    pub on_load_failure: String,
}

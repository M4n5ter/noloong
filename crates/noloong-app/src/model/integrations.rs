use super::{helpers::*, *};
use noloong_config::{
    AgentPluginDeclaration, McpPluginTransport, McpStdioTransport, McpStreamableHttpTransport,
    PluginComponent, PluginLoadFailurePolicy, SkillsPluginComponent,
};
use std::path::PathBuf;

impl AppViewModel {
    pub fn skill_root_summaries(&self) -> Vec<SkillRootSummary> {
        self.selected_profile()
            .map(|profile| {
                profile
                    .plugins
                    .iter()
                    .flat_map(|plugin| {
                        plugin
                            .components
                            .iter()
                            .filter_map(move |component| match component {
                                PluginComponent::Skills(component) => {
                                    Some(component.roots.iter().map(move |root| SkillRootSummary {
                                        plugin_id: plugin.plugin_id.clone(),
                                        plugin_name: plugin.display_name.clone(),
                                        enabled: plugin.enabled,
                                        root: root.display().to_string(),
                                    }))
                                }
                                _ => None,
                            })
                    })
                    .flatten()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn skill_root_edit(&self, index: usize) -> Option<SkillRootEdit> {
        self.skill_root_summaries()
            .get(index)
            .map(|summary| SkillRootEdit {
                plugin_id: summary.plugin_id.clone(),
                plugin_name: summary.plugin_name.clone(),
                enabled: summary.enabled,
                root: summary.root.clone(),
            })
    }

    pub fn add_skill_root(&mut self) -> usize {
        let index = self.skill_root_summaries().len();
        let next = index + 1;
        let root = PathBuf::from(if next == 1 {
            "./skills".to_string()
        } else {
            format!("./skills-{next}")
        });
        let mut appended_to_existing = false;
        {
            if let Some(profile) = self.selected_profile_mut() {
                for plugin in &mut profile.plugins {
                    for component in &mut plugin.components {
                        if let PluginComponent::Skills(component) = component {
                            component.roots.push(root.clone());
                            appended_to_existing = true;
                            break;
                        }
                    }
                    if appended_to_existing {
                        break;
                    }
                }
            }
        }
        if !appended_to_existing {
            self.push_managed_plugin_component(PluginComponent::Skills(SkillsPluginComponent {
                roots: vec![root],
            }));
        }
        self.mark_dirty_from_form();
        index
    }

    pub fn remove_skill_root(&mut self, index: usize) {
        let mut remaining = index;
        let mut removed = false;
        {
            let Some(profile) = self.selected_profile_mut() else {
                return;
            };
            'plugins: for plugin_index in 0..profile.plugins.len() {
                for component_index in 0..profile.plugins[plugin_index].components.len() {
                    let PluginComponent::Skills(component) =
                        &mut profile.plugins[plugin_index].components[component_index]
                    else {
                        continue;
                    };
                    if remaining < component.roots.len() {
                        component.roots.remove(remaining);
                        if component.roots.is_empty() {
                            profile.plugins[plugin_index]
                                .components
                                .remove(component_index);
                        }
                        removed = true;
                        break 'plugins;
                    }
                    remaining -= component.roots.len();
                }
            }
        }
        if removed {
            self.remove_empty_plugins();
            self.mark_dirty_from_form();
        }
    }

    pub fn set_skill_root(&mut self, index: usize, value: String) {
        if let Some(root) = self.skill_root_mut(index) {
            *root = PathBuf::from(value.trim());
            self.mark_dirty_from_form();
        }
    }

    pub fn mcp_server_summaries(&self) -> Vec<McpServerSummary> {
        self.selected_profile()
            .map(|profile| {
                profile
                    .plugins
                    .iter()
                    .flat_map(|plugin| {
                        plugin.components.iter().filter_map(|component| {
                            let PluginComponent::Mcp(component) = component else {
                                return None;
                            };
                            Some(McpServerSummary {
                                plugin_id: plugin.plugin_id.clone(),
                                plugin_name: plugin.display_name.clone(),
                                enabled: plugin.enabled,
                                server_id: component.server_id.clone(),
                                transport: mcp_transport_kind(&component.transport).into(),
                                endpoint: mcp_transport_endpoint(&component.transport),
                                cwd: mcp_transport_cwd(&component.transport),
                                enabled_tools: component.enabled_tools.len(),
                                disabled_tools: component.disabled_tools.len(),
                                tool_name_prefix: component
                                    .tool_name_prefix
                                    .clone()
                                    .unwrap_or_else(|| "default".into()),
                                request_timeout_secs: component.request_timeout_secs,
                                environment_count: mcp_transport_env_count(&component.transport),
                                header_count: mcp_transport_header_count(&component.transport),
                            })
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn mcp_server_edit(&self, index: usize) -> Option<McpServerEdit> {
        let component = self.mcp_component(index)?;
        Some(McpServerEdit {
            server_id: component.server_id.clone(),
            transport: mcp_transport_kind(&component.transport).into(),
            endpoint: mcp_transport_endpoint(&component.transport),
            args: match &component.transport {
                McpPluginTransport::Stdio(transport) => transport.args.join(", "),
                McpPluginTransport::StreamableHttp(_) => String::new(),
            },
            tool_name_prefix: component.tool_name_prefix.clone().unwrap_or_default(),
            enabled_tools: component.enabled_tools.join(", "),
            disabled_tools: component.disabled_tools.join(", "),
            request_timeout_secs: component
                .request_timeout_secs
                .map(|value| value.to_string())
                .unwrap_or_default(),
        })
    }

    pub fn add_mcp_stdio_server(&mut self) -> usize {
        let index = self.mcp_server_summaries().len();
        let next = self.next_mcp_server_number();
        let component = PluginComponent::Mcp(noloong_config::McpPluginComponent {
            server_id: format!("local-stdio-{next}"),
            transport: McpPluginTransport::Stdio(McpStdioTransport {
                command: "npx".into(),
                args: Vec::new(),
                cwd: None,
                env: Default::default(),
                request_timeout_secs: Some(30),
            }),
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            tool_name_prefix: Some(format!("mcp{next}")),
            request_timeout_secs: Some(30),
        });
        self.push_managed_plugin_component(component);
        self.mark_dirty_from_form();
        index
    }

    pub fn add_mcp_http_server(&mut self) -> usize {
        let index = self.mcp_server_summaries().len();
        let next = self.next_mcp_server_number();
        let component = PluginComponent::Mcp(noloong_config::McpPluginComponent {
            server_id: format!("remote-http-{next}"),
            transport: McpPluginTransport::StreamableHttp(McpStreamableHttpTransport {
                url: "https://example.com/mcp".into(),
                headers: Default::default(),
                connect_timeout_secs: Some(10),
                request_timeout_secs: Some(30),
            }),
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            tool_name_prefix: Some(format!("mcp{next}")),
            request_timeout_secs: Some(30),
        });
        self.push_managed_plugin_component(component);
        self.mark_dirty_from_form();
        index
    }

    pub fn remove_mcp_server(&mut self, index: usize) {
        let mut remaining = index;
        if let Some(profile) = self.selected_profile_mut() {
            for plugin in &mut profile.plugins {
                for component_index in 0..plugin.components.len() {
                    if matches!(plugin.components[component_index], PluginComponent::Mcp(_)) {
                        if remaining == 0 {
                            plugin.components.remove(component_index);
                            self.remove_empty_plugins();
                            self.mark_dirty_from_form();
                            return;
                        }
                        remaining -= 1;
                    }
                }
            }
        }
    }

    pub fn set_mcp_server_id(&mut self, index: usize, value: String) {
        if let Some(component) = self.mcp_component_mut(index) {
            component.server_id = value.trim().to_string();
            self.mark_dirty_from_form();
        }
    }

    pub fn set_mcp_endpoint(&mut self, index: usize, value: String) {
        if let Some(component) = self.mcp_component_mut(index) {
            match &mut component.transport {
                McpPluginTransport::Stdio(transport) => transport.command = value.trim().into(),
                McpPluginTransport::StreamableHttp(transport) => {
                    transport.url = value.trim().into()
                }
            }
            self.mark_dirty_from_form();
        }
    }

    pub fn set_mcp_args(&mut self, index: usize, value: String) {
        if let Some(component) = self.mcp_component_mut(index)
            && let McpPluginTransport::Stdio(transport) = &mut component.transport
        {
            transport.args = split_lines(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_mcp_tool_prefix(&mut self, index: usize, value: String) {
        if let Some(component) = self.mcp_component_mut(index) {
            component.tool_name_prefix = optional_string(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_mcp_enabled_tools(&mut self, index: usize, value: String) {
        if let Some(component) = self.mcp_component_mut(index) {
            component.enabled_tools = split_lines(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_mcp_disabled_tools(&mut self, index: usize, value: String) {
        if let Some(component) = self.mcp_component_mut(index) {
            component.disabled_tools = split_lines(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_mcp_timeout(&mut self, index: usize, value: String) {
        if let Some(component) = self.mcp_component_mut(index) {
            component.request_timeout_secs = optional_u64(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn switch_mcp_transport(&mut self, index: usize) {
        if let Some(component) = self.mcp_component_mut(index) {
            component.transport = match &component.transport {
                McpPluginTransport::Stdio(_) => {
                    McpPluginTransport::StreamableHttp(McpStreamableHttpTransport {
                        url: "https://example.com/mcp".into(),
                        headers: Default::default(),
                        connect_timeout_secs: Some(10),
                        request_timeout_secs: component.request_timeout_secs,
                    })
                }
                McpPluginTransport::StreamableHttp(_) => {
                    McpPluginTransport::Stdio(McpStdioTransport {
                        command: "npx".into(),
                        args: Vec::new(),
                        cwd: None,
                        env: Default::default(),
                        request_timeout_secs: component.request_timeout_secs,
                    })
                }
            };
            self.mark_dirty_from_form();
        }
    }

    pub fn extension_summaries(&self) -> Vec<ExtensionSummary> {
        self.selected_profile()
            .map(|profile| {
                profile
                    .plugins
                    .iter()
                    .flat_map(|plugin| {
                        plugin.components.iter().filter_map(|component| {
                            let PluginComponent::NoloongExtension(component) = component else {
                                return None;
                            };
                            Some(ExtensionSummary {
                                plugin_id: plugin.plugin_id.clone(),
                                plugin_name: plugin.display_name.clone(),
                                enabled: plugin.enabled,
                                transport: extension_transport_summary(&component.transport),
                                capabilities: component
                                    .allowed_capabilities
                                    .iter()
                                    .map(capability_selector_summary)
                                    .collect(),
                            })
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn plugin_overviews(&self) -> Vec<PluginOverview> {
        self.selected_profile()
            .map(|profile| profile.plugins.iter().map(plugin_overview).collect())
            .unwrap_or_default()
    }

    pub fn plugin_count(&self) -> usize {
        self.selected_profile()
            .map(|profile| profile.plugins.len())
            .unwrap_or_default()
    }

    pub fn manifest_patch_summaries(&self) -> Vec<String> {
        self.selected_profile()
            .map(|profile| {
                profile
                    .manifest_patches
                    .iter()
                    .map(|patch| patch.summary())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn metadata_rows(&self) -> Vec<(String, String)> {
        self.selected_profile()
            .map(|profile| {
                profile
                    .metadata
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn mcp_component(&self, index: usize) -> Option<&noloong_config::McpPluginComponent> {
        let mut remaining = index;
        for plugin in &self.selected_profile()?.plugins {
            for component in &plugin.components {
                let PluginComponent::Mcp(component) = component else {
                    continue;
                };
                if remaining == 0 {
                    return Some(component);
                }
                remaining -= 1;
            }
        }
        None
    }

    fn skill_root_mut(&mut self, index: usize) -> Option<&mut PathBuf> {
        let mut remaining = index;
        for plugin in &mut self.selected_profile_mut()?.plugins {
            for component in &mut plugin.components {
                let PluginComponent::Skills(component) = component else {
                    continue;
                };
                for root in &mut component.roots {
                    if remaining == 0 {
                        return Some(root);
                    }
                    remaining -= 1;
                }
            }
        }
        None
    }

    fn mcp_component_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut noloong_config::McpPluginComponent> {
        let mut remaining = index;
        for plugin in &mut self.selected_profile_mut()?.plugins {
            for component in &mut plugin.components {
                let PluginComponent::Mcp(component) = component else {
                    continue;
                };
                if remaining == 0 {
                    return Some(component);
                }
                remaining -= 1;
            }
        }
        None
    }

    fn next_mcp_server_number(&self) -> usize {
        self.mcp_server_summaries().len() + 1
    }

    fn push_managed_plugin_component(&mut self, component: PluginComponent) {
        let Some(profile) = self.selected_profile_mut() else {
            return;
        };
        let plugin_index = profile
            .plugins
            .iter()
            .position(|plugin| plugin.plugin_id == "local-integrations");
        let plugin = if let Some(plugin_index) = plugin_index {
            &mut profile.plugins[plugin_index]
        } else {
            profile.plugins.push(AgentPluginDeclaration {
                plugin_id: "local-integrations".into(),
                display_name: "Local integrations".into(),
                description: Some("Managed by the Noloong settings app.".into()),
                components: Vec::new(),
                enabled: true,
                on_load_failure: PluginLoadFailurePolicy::DisableForRun,
            });
            profile.plugins.last_mut().expect("plugin was just pushed")
        };
        plugin.components.push(component);
    }

    fn remove_empty_plugins(&mut self) {
        if let Some(profile) = self.selected_profile_mut() {
            profile
                .plugins
                .retain(|plugin| !plugin.components.is_empty());
        }
    }
}

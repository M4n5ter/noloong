use noloong_agent::manifest::ManifestError;
use noloong_agent::{
    AgentManifest, AgentPluginDeclaration, AgentSystemPrompt, ApprovalPolicy,
    BuiltInSystemPromptProfile, BuiltInToolName, FileEditToolPolicy, Locale, ManifestPatch,
    ManifestProposalStore, NoloongExtensionPluginComponent, NoloongExtensionTransport,
    PluginComponent, PluginEnvSource, PluginLoadFailurePolicy, StdioPluginTransport,
    SystemPromptAddition, built_in_system_prompt,
};
use noloong_agent_core::ExtensionCapabilitySelector;
use std::collections::BTreeMap;

#[test]
fn manifest_patch_applies_prompt_tools_policy() {
    let mut manifest = AgentManifest::default();

    manifest
        .apply_patch(ManifestPatch::ReplaceSystemPrompt {
            prompt: "New prompt".into(),
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::SetLocale { locale: Locale::Zh })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::EnableTool {
            tool_name: BuiltInToolName::HostExecStart,
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::UpdateApprovalPolicy {
            policy: ApprovalPolicy::AllowAll,
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::UpdateFileEditToolPolicy {
            policy: FileEditToolPolicy::ApplyPatch,
        })
        .unwrap();

    assert_eq!(
        manifest.system_prompt,
        AgentSystemPrompt::custom("New prompt")
    );
    assert_eq!(manifest.effective_system_prompt(), "New prompt");
    assert_eq!(manifest.locale, Locale::Zh);
    assert!(
        manifest
            .enabled_tools
            .contains(&BuiltInToolName::HostExecStart)
    );
    assert_eq!(manifest.approval_policy, ApprovalPolicy::AllowAll);
    assert_eq!(
        manifest.file_edit_tool_policy,
        FileEditToolPolicy::ApplyPatch
    );
}

#[test]
fn manifest_default_system_prompt_is_locale_selected_builtin() {
    let mut manifest = AgentManifest::default();
    let value = serde_json::to_value(&manifest).unwrap();

    assert_eq!(
        value["systemPrompt"],
        serde_json::json!({"source": "built_in"})
    );
    assert_eq!(
        manifest.effective_system_prompt(),
        built_in_system_prompt(Locale::En)
    );

    manifest
        .apply_patch(ManifestPatch::SetLocale { locale: Locale::Zh })
        .unwrap();

    assert_eq!(
        manifest.effective_system_prompt(),
        built_in_system_prompt(Locale::Zh)
    );
}

#[test]
fn manifest_custom_system_prompt_does_not_follow_locale() {
    let mut manifest = AgentManifest::default();

    manifest
        .apply_patch(ManifestPatch::ReplaceSystemPrompt {
            prompt: "Custom prompt".into(),
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::SetLocale { locale: Locale::Zh })
        .unwrap();

    assert_eq!(manifest.effective_system_prompt(), "Custom prompt");
}

#[test]
fn manifest_patch_restores_built_in_system_prompt() {
    let mut manifest = AgentManifest::default();

    manifest
        .apply_patch(ManifestPatch::ReplaceSystemPrompt {
            prompt: "Custom prompt".into(),
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::UseBuiltInSystemPrompt)
        .unwrap();

    assert_eq!(manifest.system_prompt, AgentSystemPrompt::default());
    assert_eq!(
        manifest.effective_system_prompt(),
        built_in_system_prompt(Locale::En)
    );
}

#[test]
fn manifest_system_prompt_additions_are_crud_and_ordered() {
    let mut manifest = AgentManifest::default();

    manifest
        .apply_patch(ManifestPatch::UpsertSystemPromptAddition {
            addition: SystemPromptAddition::new("channel.telegram", "Use Telegram."),
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::UpsertSystemPromptAddition {
            addition: SystemPromptAddition::new("workspace", "Use the current workspace."),
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::SetSystemPromptAdditionEnabled {
            id: "channel.telegram".into(),
            enabled: false,
        })
        .unwrap();
    manifest
        .apply_patch(ManifestPatch::ReorderSystemPromptAdditions {
            ids: vec!["workspace".into(), "channel.telegram".into()],
        })
        .unwrap();

    let additions = manifest.system_prompt.additions();
    assert_eq!(additions[0].id, "workspace");
    assert_eq!(additions[1].id, "channel.telegram");
    assert!(!additions[1].enabled);
    assert!(
        manifest
            .effective_system_prompt()
            .contains("Use the current workspace.")
    );
    assert!(!manifest.effective_system_prompt().contains("Use Telegram."));

    manifest
        .apply_patch(ManifestPatch::RemoveSystemPromptAddition {
            id: "channel.telegram".into(),
        })
        .unwrap();
    assert_eq!(manifest.system_prompt.additions().len(), 1);

    manifest
        .apply_patch(ManifestPatch::ClearSystemPromptAdditions)
        .unwrap();
    assert!(manifest.system_prompt.additions().is_empty());
}

#[test]
fn manifest_system_prompt_base_changes_preserve_additions() {
    let mut manifest = AgentManifest::default();
    manifest
        .apply_patch(ManifestPatch::UpsertSystemPromptAddition {
            addition: SystemPromptAddition::new("channel.<telegram>&\"", "Use Telegram."),
        })
        .unwrap();

    manifest
        .apply_patch(ManifestPatch::ReplaceSystemPrompt {
            prompt: "Custom base.".into(),
        })
        .unwrap();
    assert_eq!(
        manifest.effective_system_prompt(),
        "Custom base.\n\n<system_prompt_additions>\n<addition id=\"channel.&lt;telegram&gt;&amp;&quot;\">\nUse Telegram.\n</addition>\n</system_prompt_additions>\n"
    );

    manifest
        .apply_patch(ManifestPatch::UseBuiltInSystemPrompt)
        .unwrap();
    assert_eq!(manifest.system_prompt.additions().len(), 1);

    manifest
        .apply_patch(ManifestPatch::SetBuiltInSystemPromptProfile {
            profile: BuiltInSystemPromptProfile::OpenAi,
        })
        .unwrap();
    match manifest.system_prompt {
        AgentSystemPrompt::BuiltIn { profile, .. } => {
            assert_eq!(profile, BuiltInSystemPromptProfile::OpenAi)
        }
        AgentSystemPrompt::Custom { .. } => panic!("expected built-in prompt"),
    }
}

#[test]
fn manifest_file_edit_policy_round_trips_as_snake_case() {
    let manifest =
        AgentManifest::default().with_file_edit_tool_policy(FileEditToolPolicy::WriteFile);
    let value = serde_json::to_value(&manifest).unwrap();

    assert_eq!(value["fileEditToolPolicy"], "write_file");

    for (json_value, expected) in [
        ("auto_by_model", FileEditToolPolicy::AutoByModel),
        ("apply_patch", FileEditToolPolicy::ApplyPatch),
        ("write_file", FileEditToolPolicy::WriteFile),
        ("disabled", FileEditToolPolicy::Disabled),
    ] {
        let manifest: AgentManifest = serde_json::from_value(serde_json::json!({
            "locale": "en",
            "systemPrompt": {"source": "custom", "prompt": "test"},
            "fileEditToolPolicy": json_value,
            "approvalPolicy": {"mode": "require_approval"}
        }))
        .unwrap();
        assert_eq!(manifest.file_edit_tool_policy, expected);
    }
}

#[test]
fn manifest_default_file_edit_policy_is_auto_by_model() {
    assert_eq!(
        AgentManifest::default().file_edit_tool_policy,
        FileEditToolPolicy::AutoByModel
    );
}

#[test]
fn manifest_default_enables_built_in_tools() {
    let manifest = AgentManifest::default();

    for tool_name in BuiltInToolName::ALL {
        assert!(manifest.enabled_tools.contains(tool_name), "{tool_name}");
    }
}

#[test]
fn manifest_missing_enabled_tools_uses_default_built_ins() {
    let manifest: AgentManifest = serde_json::from_value(serde_json::json!({
        "locale": "en",
        "systemPrompt": {"source": "custom", "prompt": "test"},
        "approvalPolicy": {"mode": "require_approval"}
    }))
    .unwrap();

    assert_eq!(manifest.enabled_tools, BuiltInToolName::default_enabled());
}

#[test]
fn manifest_explicit_empty_enabled_tools_disables_built_ins() {
    let manifest: AgentManifest = serde_json::from_value(serde_json::json!({
        "locale": "en",
        "systemPrompt": {"source": "custom", "prompt": "test"},
        "enabledTools": [],
        "approvalPolicy": {"mode": "require_approval"}
    }))
    .unwrap();

    assert!(manifest.enabled_tools.is_empty());
}

#[test]
fn manifest_patch_rejects_invalid_changes() {
    let mut manifest = AgentManifest::default();
    let before = manifest.clone();

    let error = manifest
        .apply_patch(ManifestPatch::ReplaceSystemPrompt { prompt: " ".into() })
        .unwrap_err();

    assert_eq!(manifest, before);
    assert!(error.to_string().contains("system prompt"));
}

#[test]
fn manifest_patch_rejects_unknown_tool_names() {
    let error = serde_json::from_value::<ManifestPatch>(serde_json::json!({
        "op": "enable_tool",
        "toolName": "host.exec.unknown"
    }))
    .unwrap_err();

    assert!(error.to_string().contains("unknown built-in tool"));
}

#[test]
fn manifest_phase_patch_is_reserved() {
    let mut manifest = AgentManifest::default();

    let error = manifest
        .apply_patch(ManifestPatch::ReservedPhaseProfile {
            description: "replace turn decision".into(),
            metadata: serde_json::json!({}),
        })
        .unwrap_err();

    assert!(error.to_string().contains("reserved"));
}

#[test]
fn manifest_proposal_store_records_without_applying() {
    let store = ManifestProposalStore::default();
    let manifest = AgentManifest::default();

    let proposal = store
        .record_pending_proposal(ManifestPatch::DisableTool {
            tool_name: BuiltInToolName::HostExecStart,
        })
        .unwrap();

    assert_eq!(store.pending_len(), 1);
    assert_eq!(store.approved_len(), 0);
    assert_eq!(proposal.summary, "disable tool host.exec.start");
    assert!(
        manifest
            .enabled_tools
            .contains(&BuiltInToolName::HostExecStart)
    );
}

#[test]
fn manifest_proposal_store_approves_pending_proposals() {
    let store = ManifestProposalStore::default();
    let proposal = store
        .record_pending_proposal(ManifestPatch::EnableTool {
            tool_name: BuiltInToolName::HostExecStart,
        })
        .unwrap();

    let approved = store.approve_proposal(&proposal.proposal_id).unwrap();

    assert_eq!(approved.proposal_id, proposal.proposal_id);
    assert_eq!(store.pending_len(), 0);
    assert_eq!(store.approved_len(), 1);
}

#[test]
fn manifest_plugin_declaration_round_trips_and_defaults() {
    let manifest: AgentManifest = serde_json::from_value(serde_json::json!({
        "locale": "en",
        "systemPrompt": {"source": "custom", "prompt": "test"},
        "approvalPolicy": {"mode": "require_approval"},
        "plugins": {
            "echo": {
                "pluginId": "echo",
                "displayName": "Echo",
                "components": [
                    {
                        "type": "noloong_extension",
                        "transport": {
                            "type": "stdio",
                            "command": "node",
                            "args": ["examples/extensions/echo.mjs"],
                            "env": {
                                "OPENAI_API_KEY": {
                                    "type": "host_env",
                                    "name": "OPENAI_API_KEY"
                                }
                            }
                        },
                        "allowedCapabilities": [
                            {"type": "tool", "name": "echo.run"}
                        ]
                    },
                    {
                        "type": "skills",
                        "roots": ["./skills"]
                    },
                    {
                        "type": "mcp",
                        "serverId": "docs",
                        "transport": {
                            "type": "streamable_http",
                            "url": "https://example.com/mcp",
                            "headers": {
                                "Authorization": {
                                    "type": "host_env",
                                    "name": "DOCS_TOKEN",
                                    "prefix": "Bearer "
                                }
                            }
                        }
                    }
                ],
                "enabled": true
            }
        }
    }))
    .unwrap();

    manifest.validate().unwrap();
    let plugin = &manifest.plugins["echo"];
    assert!(plugin.enabled);
    assert_eq!(
        plugin.on_load_failure,
        PluginLoadFailurePolicy::DisableForRun
    );
    assert_eq!(
        extension_component(plugin).allowed_capabilities,
        vec![ExtensionCapabilitySelector::Tool {
            name: "echo.run".into(),
        }]
    );
    assert_eq!(plugin.components.len(), 3);
}

#[test]
fn manifest_plugin_patches_apply_and_reject_invalid_state() {
    let mut manifest = AgentManifest::default();
    let plugin = test_plugin("echo");

    manifest
        .apply_patch(ManifestPatch::RegisterPlugin {
            plugin: plugin.clone(),
        })
        .unwrap();
    assert_eq!(manifest.plugins["echo"], plugin);

    let duplicate = manifest
        .apply_patch(ManifestPatch::RegisterPlugin { plugin })
        .unwrap_err();
    assert!(duplicate.to_string().contains("already exists"));

    manifest
        .apply_patch(ManifestPatch::SetPluginEnabled {
            plugin_id: "echo".into(),
            enabled: false,
        })
        .unwrap();
    assert!(!manifest.plugins["echo"].enabled);

    manifest
        .apply_patch(ManifestPatch::RemovePlugin {
            plugin_id: "echo".into(),
        })
        .unwrap();
    assert!(!manifest.plugins.contains_key("echo"));

    let missing = manifest
        .apply_patch(ManifestPatch::SetPluginEnabled {
            plugin_id: "echo".into(),
            enabled: true,
        })
        .unwrap_err();
    assert!(missing.to_string().contains("unknown plugin"));
}

#[test]
fn manifest_plugin_patch_summary_is_auditable_without_secret_values() {
    let mut plugin = test_plugin("auth");
    let NoloongExtensionTransport::Stdio(transport) =
        &mut extension_component_mut(&mut plugin).transport;
    transport.env.insert(
        "API_KEY".into(),
        PluginEnvSource::HostEnv {
            name: "SECRET_API_KEY".into(),
        },
    );

    let summary = ManifestPatch::RegisterPlugin { plugin }.summary();

    assert!(summary.contains("register plugin auth"));
    assert!(summary.contains("node"));
    assert!(summary.contains("SECRET_API_KEY"));
    assert!(!summary.contains("secret-value"));
}

#[test]
fn manifest_plugin_validation_rejects_empty_ids_and_commands() {
    let mut plugin = test_plugin("echo");
    plugin.plugin_id = " ".into();
    let error = ManifestPatch::RegisterPlugin { plugin }
        .validate()
        .unwrap_err();
    assert!(error.to_string().contains("pluginId"));

    let mut plugin = test_plugin("echo");
    let NoloongExtensionTransport::Stdio(transport) =
        &mut extension_component_mut(&mut plugin).transport;
    transport.command = " ".into();
    let error = ManifestPatch::RegisterPlugin { plugin }
        .validate()
        .unwrap_err();
    assert!(error.to_string().contains("command"));

    let mut plugin = test_plugin("echo");
    let first_capability = extension_component(&plugin).allowed_capabilities[0].clone();
    extension_component_mut(&mut plugin)
        .allowed_capabilities
        .push(first_capability);
    let error = ManifestPatch::RegisterPlugin { plugin }
        .validate()
        .unwrap_err();
    assert_eq!(
        error,
        ManifestError::Invalid("duplicate allowed capability: tool:echo.run".into())
    );
}

#[test]
fn manifest_plugin_validation_rejects_zero_timeouts() {
    let mut plugin = test_plugin("echo");
    let NoloongExtensionTransport::Stdio(transport) =
        &mut extension_component_mut(&mut plugin).transport;
    transport.request_timeout_secs = Some(0);
    let error = ManifestPatch::RegisterPlugin { plugin }
        .validate()
        .unwrap_err();
    assert_eq!(
        error,
        ManifestError::Invalid("requestTimeoutSecs must be greater than zero".into())
    );

    let mut plugin = test_plugin("echo");
    let NoloongExtensionTransport::Stdio(transport) =
        &mut extension_component_mut(&mut plugin).transport;
    transport.stream_timeout_secs = Some(0);
    let error = ManifestPatch::RegisterPlugin { plugin }
        .validate()
        .unwrap_err();
    assert_eq!(
        error,
        ManifestError::Invalid("streamTimeoutSecs must be greater than zero".into())
    );
}

fn test_plugin(plugin_id: &str) -> AgentPluginDeclaration {
    AgentPluginDeclaration {
        plugin_id: plugin_id.into(),
        display_name: "Echo".into(),
        description: None,
        components: vec![PluginComponent::NoloongExtension(
            NoloongExtensionPluginComponent {
                transport: NoloongExtensionTransport::Stdio(StdioPluginTransport {
                    command: "node".into(),
                    args: vec!["examples/extensions/echo.mjs".into()],
                    cwd: None,
                    env: BTreeMap::new(),
                    request_timeout_secs: None,
                    stream_timeout_secs: None,
                }),
                allowed_capabilities: vec![ExtensionCapabilitySelector::Tool {
                    name: "echo.run".into(),
                }],
            },
        )],
        enabled: true,
        on_load_failure: PluginLoadFailurePolicy::DisableForRun,
    }
}

fn extension_component(plugin: &AgentPluginDeclaration) -> &NoloongExtensionPluginComponent {
    plugin
        .components
        .iter()
        .find_map(|component| match component {
            PluginComponent::NoloongExtension(component) => Some(component),
            _ => None,
        })
        .expect("test plugin has noloong extension component")
}

fn extension_component_mut(
    plugin: &mut AgentPluginDeclaration,
) -> &mut NoloongExtensionPluginComponent {
    plugin
        .components
        .iter_mut()
        .find_map(|component| match component {
            PluginComponent::NoloongExtension(component) => Some(component),
            _ => None,
        })
        .expect("test plugin has noloong extension component")
}

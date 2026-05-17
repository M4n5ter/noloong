use noloong_agent::{
    AgentManifest, AgentPluginDeclaration, AgentSession, NoloongExtensionPluginComponent,
    NoloongExtensionTransport, PluginComponent, PluginEnvSource, PluginLoadFailurePolicy,
    StdioPluginTransport,
};
use noloong_agent_core::{
    BoxFuture, CancellationToken, ExtensionCapabilitySelector, ModelProvider, ModelRequest,
    ModelStreamEvent, ModelStreamSink, Result, StopReason,
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[tokio::test]
async fn manifest_enabled_plugin_registers_allowed_tool() -> Result<()> {
    let manifest = AgentManifest::default()
        .with_plugin(conformance_plugin(
            "echo",
            vec![ExtensionCapabilitySelector::Tool {
                name: "conformance_echo".into(),
            }],
            PluginLoadFailurePolicy::FailRun,
        ))
        .unwrap();
    let session = AgentSession::builder().with_manifest(manifest).build();

    let runtime = session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .with_manifest_plugins()
        .await?
        .build()?;

    assert!(runtime.tool("conformance_echo").is_ok());
    Ok(())
}

#[tokio::test]
async fn manifest_disabled_plugin_does_not_start_process() -> Result<()> {
    let mut plugin = conformance_plugin(
        "disabled",
        vec![ExtensionCapabilitySelector::Tool {
            name: "conformance_echo".into(),
        }],
        PluginLoadFailurePolicy::FailRun,
    );
    plugin.enabled = false;
    extension_component_mut(&mut plugin).transport =
        NoloongExtensionTransport::Stdio(StdioPluginTransport {
            command: "missing-noloong-plugin-command".into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            request_timeout_secs: Some(1),
            stream_timeout_secs: Some(1),
        });
    let manifest = AgentManifest::default().with_plugin(plugin).unwrap();
    let session = AgentSession::builder().with_manifest(manifest).build();

    let runtime = session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .with_manifest_plugins()
        .await?
        .build()?;

    assert!(runtime.tool("conformance_echo").is_err());
    Ok(())
}

#[tokio::test]
async fn plugin_load_failure_policy_controls_runtime_build() {
    let manifest = AgentManifest::default()
        .with_plugin(missing_command_plugin(
            "soft-fail",
            PluginLoadFailurePolicy::DisableForRun,
        ))
        .unwrap();
    let session = AgentSession::builder().with_manifest(manifest).build();

    let builder = session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .with_manifest_plugins()
        .await
        .unwrap();
    assert_eq!(builder.plugin_load_warnings().len(), 1);
    builder.build().unwrap();

    let manifest = AgentManifest::default()
        .with_plugin(missing_command_plugin(
            "hard-fail",
            PluginLoadFailurePolicy::FailRun,
        ))
        .unwrap();
    let session = AgentSession::builder().with_manifest(manifest).build();

    let error = match session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .with_manifest_plugins()
        .await
    {
        Ok(_) => panic!("hard-fail plugin should fail runtime build"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("missing-noloong-plugin-command"));
}

#[tokio::test]
async fn plugin_missing_host_env_is_diagnostic() {
    let mut plugin = conformance_plugin(
        "missing-env",
        vec![ExtensionCapabilitySelector::Tool {
            name: "conformance_echo".into(),
        }],
        PluginLoadFailurePolicy::FailRun,
    );
    let NoloongExtensionTransport::Stdio(transport) =
        &mut extension_component_mut(&mut plugin).transport;
    transport.env.insert(
        "PLUGIN_API_KEY".into(),
        PluginEnvSource::HostEnv {
            name: "NOLOONG_PLUGIN_ENV_SHOULD_NOT_EXIST".into(),
        },
    );
    let manifest = AgentManifest::default().with_plugin(plugin).unwrap();
    let session = AgentSession::builder().with_manifest(manifest).build();

    let error = match session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .with_manifest_plugins()
        .await
    {
        Ok(_) => panic!("missing env should fail runtime build"),
        Err(error) => error,
    };

    let message = error.to_string();
    assert!(message.contains("NOLOONG_PLUGIN_ENV_SHOULD_NOT_EXIST"));
    assert!(message.contains("PLUGIN_API_KEY"));
}

fn conformance_plugin(
    plugin_id: &str,
    allowed_capabilities: Vec<ExtensionCapabilitySelector>,
    on_load_failure: PluginLoadFailurePolicy,
) -> AgentPluginDeclaration {
    AgentPluginDeclaration {
        plugin_id: plugin_id.into(),
        display_name: "Conformance".into(),
        description: None,
        components: vec![PluginComponent::NoloongExtension(
            NoloongExtensionPluginComponent {
                transport: NoloongExtensionTransport::Stdio(StdioPluginTransport {
                    command: "node".into(),
                    args: vec![
                        conformance_fixture().to_string_lossy().into_owned(),
                        "--mode=all-capabilities".into(),
                    ],
                    cwd: None,
                    env: BTreeMap::from([(
                        "PATH".into(),
                        PluginEnvSource::HostEnv {
                            name: "PATH".into(),
                        },
                    )]),
                    request_timeout_secs: Some(2),
                    stream_timeout_secs: Some(2),
                }),
                allowed_capabilities,
            },
        )],
        enabled: true,
        on_load_failure,
    }
}

fn missing_command_plugin(
    plugin_id: &str,
    on_load_failure: PluginLoadFailurePolicy,
) -> AgentPluginDeclaration {
    AgentPluginDeclaration {
        plugin_id: plugin_id.into(),
        display_name: "Missing".into(),
        description: None,
        components: vec![PluginComponent::NoloongExtension(
            NoloongExtensionPluginComponent {
                transport: NoloongExtensionTransport::Stdio(StdioPluginTransport {
                    command: "missing-noloong-plugin-command".into(),
                    args: Vec::new(),
                    cwd: None,
                    env: BTreeMap::new(),
                    request_timeout_secs: Some(1),
                    stream_timeout_secs: Some(1),
                }),
                allowed_capabilities: Vec::new(),
            },
        )],
        enabled: true,
        on_load_failure,
    }
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

fn conformance_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("noloong-agent-core")
        .join("tests")
        .join("fixtures")
        .join("jsonrpc-conformance-extension.mjs")
}

struct DummyModelProvider;

impl ModelProvider for DummyModelProvider {
    fn id(&self) -> &str {
        "dummy-model"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        sink: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let event = ModelStreamEvent::Finished {
                stop_reason: StopReason::Stop,
            };
            sink(event.clone()).await?;
            Ok(vec![event])
        })
    }
}

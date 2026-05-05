use crate::{
    AgentManifest, Catalog, HostEnvironment, HostProcessCompletion, HostProcessEvent,
    HostProcessManager, HostProcessSubscription, ManifestProposalStore, ProductApprovalHook,
    ProductToolName, ProductToolOutputOverflowHook, ToolOutputOverflowConfig, text,
    tools::{
        HostExecListTool, HostExecReadTool, HostExecStartTool, HostExecTerminateTool,
        HostExecWaitTool, HostExecWriteTool, ManifestPatchProposalTool,
    },
};
use noloong_agent_core::{
    Agent, AgentMessage, AgentRuntime, AgentRuntimeBuilder, Result, ToolProvider,
};
use serde_json::{Map, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub const DEFAULT_BACKGROUND_COMPLETION_PREVIEW_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub struct AgentSession {
    inner: Arc<AgentSessionInner>,
}

struct AgentSessionInner {
    manifest: Mutex<AgentManifest>,
    environment: HostEnvironment,
    process_manager: HostProcessManager,
    proposal_store: ManifestProposalStore,
    tool_output_overflow_config: ToolOutputOverflowConfig,
}

#[derive(Default)]
pub struct AgentSessionBuilder {
    manifest: AgentManifest,
    environment: Option<HostEnvironment>,
    process_manager: Option<HostProcessManager>,
    proposal_store: ManifestProposalStore,
    tool_output_overflow_config: ToolOutputOverflowConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackgroundCompletionConfig {
    pub max_preview_bytes: usize,
}

impl Default for BackgroundCompletionConfig {
    fn default() -> Self {
        Self {
            max_preview_bytes: DEFAULT_BACKGROUND_COMPLETION_PREVIEW_BYTES,
        }
    }
}

#[derive(Debug)]
pub struct BackgroundCompletionSteering {
    _subscription: HostProcessSubscription,
}

impl AgentSession {
    pub fn builder() -> AgentSessionBuilder {
        AgentSessionBuilder::default()
    }

    pub fn manifest(&self) -> AgentManifest {
        self.inner
            .manifest
            .lock()
            .expect("agent session manifest lock poisoned")
            .clone()
    }

    pub fn host_environment(&self) -> &HostEnvironment {
        &self.inner.environment
    }

    pub fn process_manager(&self) -> HostProcessManager {
        self.inner.process_manager.clone()
    }

    pub fn proposal_store(&self) -> ManifestProposalStore {
        self.inner.proposal_store.clone()
    }

    pub fn apply_approved_manifest_patches(&self) -> Result<Vec<String>> {
        let proposals = self.inner.proposal_store.drain_approved();
        let mut manifest = self
            .inner
            .manifest
            .lock()
            .expect("agent session manifest lock poisoned");
        let mut applied = Vec::with_capacity(proposals.len());
        for proposal in proposals {
            manifest
                .apply_patch(proposal.patch)
                .map_err(|error| noloong_agent_core::AgentCoreError::Provider(error.to_string()))?;
            applied.push(proposal.proposal_id);
        }
        Ok(applied)
    }

    pub fn runtime_builder(&self) -> AgentRuntimeBuilder {
        let manifest = self.manifest();
        let catalog = Catalog::new(manifest.locale);
        let mut builder = AgentRuntime::builder()
            .with_context_provider(Arc::new(ProductHostContextProvider::new(
                self.inner.environment.clone(),
                catalog.clone(),
            )))
            .with_tool_hook(Arc::new(ProductApprovalHook::new(
                manifest.approval_policy.clone(),
                catalog.clone(),
            )))
            .with_tool_hook(Arc::new(
                ProductToolOutputOverflowHook::new(self.inner.tool_output_overflow_config.clone())
                    .with_catalog(catalog.clone()),
            ));
        for tool in self.tools_for_manifest(&manifest, &catalog) {
            builder = builder.with_tool(tool);
        }
        builder
    }

    pub fn attach_background_completion_steering(
        &self,
        agent: &Agent,
        config: BackgroundCompletionConfig,
    ) -> BackgroundCompletionSteering {
        let agent = agent.clone();
        let inner = Arc::clone(&self.inner);
        let subscription = self.inner.process_manager.subscribe(move |event| {
            let HostProcessEvent::JobCompleted { completion } = event;
            let locale = inner
                .manifest
                .lock()
                .expect("agent session manifest lock poisoned")
                .locale;
            let catalog = Catalog::new(locale);
            agent.steer(completion_message(&completion, &config, &catalog));
        });
        BackgroundCompletionSteering {
            _subscription: subscription,
        }
    }

    fn tools_for_manifest(
        &self,
        manifest: &AgentManifest,
        catalog: &Catalog,
    ) -> Vec<Arc<dyn ToolProvider>> {
        manifest
            .enabled_tools
            .iter()
            .map(|name| self.tool_for_name(*name, catalog))
            .collect()
    }

    fn tool_for_name(&self, name: ProductToolName, catalog: &Catalog) -> Arc<dyn ToolProvider> {
        match name {
            ProductToolName::HostExecStart => Arc::new(HostExecStartTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            )),
            ProductToolName::HostExecRead => Arc::new(HostExecReadTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            )),
            ProductToolName::HostExecWait => Arc::new(HostExecWaitTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            )),
            ProductToolName::HostExecWrite => Arc::new(HostExecWriteTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            )),
            ProductToolName::HostExecTerminate => Arc::new(HostExecTerminateTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            )),
            ProductToolName::HostExecList => Arc::new(HostExecListTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            )),
            ProductToolName::ManifestProposePatch => Arc::new(ManifestPatchProposalTool::new(
                self.inner.proposal_store.clone(),
                catalog.clone(),
            )),
        }
    }
}

fn completion_message(
    completion: &HostProcessCompletion,
    config: &BackgroundCompletionConfig,
    catalog: &Catalog,
) -> AgentMessage {
    let job_id = &completion.snapshot.job_id;
    let output_preview = output_preview_text(completion, config.max_preview_bytes, catalog);
    let text = catalog.render_background_completion(completion, &output_preview);
    let mut message = AgentMessage::user(format!("host-exec-completed-{job_id}"), text);
    message.metadata = completion_metadata(completion);
    message
}

fn completion_metadata(completion: &HostProcessCompletion) -> Map<String, serde_json::Value> {
    let mut metadata = Map::new();
    metadata.insert("noloong.kind".into(), json!("host.exec.completed"));
    metadata.insert("jobId".into(), json!(completion.snapshot.job_id));
    metadata.insert("status".into(), json!(completion.snapshot.status));
    metadata.insert("nextCursor".into(), json!(completion.output.next_cursor));
    metadata.insert(
        "droppedBeforeSeq".into(),
        json!(completion.output.dropped_before_seq),
    );
    metadata
}

fn output_preview_text(
    completion: &HostProcessCompletion,
    max_bytes: usize,
    catalog: &Catalog,
) -> String {
    if completion.output.chunks.is_empty() {
        return catalog.no_buffered_output().into();
    }
    let mut text = String::new();
    for chunk in &completion.output.chunks {
        text.push('[');
        text.push_str(catalog.render_process_stream(chunk.stream));
        text.push_str("] ");
        text.push_str(&chunk.text);
        if !chunk.text.ends_with('\n') {
            text.push('\n');
        }
    }
    text::suffix_to_bytes(&text, max_bytes.max(1))
}

impl AgentSessionBuilder {
    pub fn with_manifest(mut self, manifest: AgentManifest) -> Self {
        self.manifest = manifest;
        self
    }

    pub fn with_environment(mut self, environment: HostEnvironment) -> Self {
        self.environment = Some(environment);
        self
    }

    pub fn with_process_manager(mut self, process_manager: HostProcessManager) -> Self {
        self.process_manager = Some(process_manager);
        self
    }

    pub fn with_tool_output_overflow_config(mut self, config: ToolOutputOverflowConfig) -> Self {
        self.tool_output_overflow_config = config;
        self
    }

    pub fn with_max_inline_tool_output_bytes(mut self, max_inline_bytes: usize) -> Self {
        self.tool_output_overflow_config.max_inline_bytes = max_inline_bytes;
        self
    }

    pub fn with_tool_output_temp_dir(mut self, temp_dir: impl Into<PathBuf>) -> Self {
        self.tool_output_overflow_config.temp_dir = temp_dir.into();
        self
    }

    pub fn build(self) -> AgentSession {
        let environment = self
            .environment
            .unwrap_or_else(|| HostEnvironment::detect(Some(self.manifest.locale)));
        AgentSession {
            inner: Arc::new(AgentSessionInner {
                manifest: Mutex::new(self.manifest),
                environment,
                process_manager: self.process_manager.unwrap_or_default(),
                proposal_store: self.proposal_store,
                tool_output_overflow_config: self.tool_output_overflow_config,
            }),
        }
    }
}

struct ProductHostContextProvider {
    environment: HostEnvironment,
    catalog: Catalog,
}

impl ProductHostContextProvider {
    fn new(environment: HostEnvironment, catalog: Catalog) -> Self {
        Self {
            environment,
            catalog,
        }
    }
}

impl noloong_agent_core::ContextProvider for ProductHostContextProvider {
    fn id(&self) -> &str {
        "noloong.product.host-context"
    }

    fn prepare_context<'a>(
        &'a self,
        _request: noloong_agent_core::ContextRequest,
        cancellation: noloong_agent_core::CancellationToken,
    ) -> noloong_agent_core::BoxFuture<'a, Vec<noloong_agent_core::AgentEffect>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            Ok(vec![noloong_agent_core::AgentEffect::PatchContext {
                patch: noloong_agent_core::ContextPatch::Set {
                    key: "noloong.host.environment".into(),
                    value: serde_json::json!({
                        "text": self.catalog.render_host_environment(&self.environment),
                        "environment": self.environment,
                    }),
                },
            }])
        })
    }
}

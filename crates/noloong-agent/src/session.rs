use crate::{
    AgentManifest, Catalog, HostEnvironment, HostProcessManager, ManifestProposalStore,
    ProductApprovalHook, ProductToolName,
    tools::{
        HostExecListTool, HostExecReadTool, HostExecStartTool, HostExecTerminateTool,
        HostExecWaitTool, HostExecWriteTool, ManifestPatchProposalTool,
    },
};
use noloong_agent_core::{AgentRuntime, AgentRuntimeBuilder, Result, ToolProvider};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AgentSession {
    inner: Arc<AgentSessionInner>,
}

struct AgentSessionInner {
    manifest: Mutex<AgentManifest>,
    environment: HostEnvironment,
    process_manager: HostProcessManager,
    proposal_store: ManifestProposalStore,
}

#[derive(Default)]
pub struct AgentSessionBuilder {
    manifest: AgentManifest,
    environment: Option<HostEnvironment>,
    process_manager: Option<HostProcessManager>,
    proposal_store: ManifestProposalStore,
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
            )));
        for tool in self.tools_for_manifest(&manifest, &catalog) {
            builder = builder.with_tool(tool);
        }
        builder
    }

    fn tools_for_manifest(
        &self,
        manifest: &AgentManifest,
        catalog: &Catalog,
    ) -> Vec<Arc<dyn ToolProvider>> {
        let mut tools: Vec<Arc<dyn ToolProvider>> = Vec::new();
        let manager = self.inner.process_manager.clone();
        let proposal_store = self.inner.proposal_store.clone();
        for name in &manifest.enabled_tools {
            match name {
                ProductToolName::HostExecStart => {
                    tools.push(Arc::new(HostExecStartTool::new(
                        manager.clone(),
                        catalog.clone(),
                    )));
                }
                ProductToolName::HostExecRead => {
                    tools.push(Arc::new(HostExecReadTool::new(
                        manager.clone(),
                        catalog.clone(),
                    )));
                }
                ProductToolName::HostExecWait => {
                    tools.push(Arc::new(HostExecWaitTool::new(
                        manager.clone(),
                        catalog.clone(),
                    )));
                }
                ProductToolName::HostExecWrite => {
                    tools.push(Arc::new(HostExecWriteTool::new(
                        manager.clone(),
                        catalog.clone(),
                    )));
                }
                ProductToolName::HostExecTerminate => {
                    tools.push(Arc::new(HostExecTerminateTool::new(
                        manager.clone(),
                        catalog.clone(),
                    )));
                }
                ProductToolName::HostExecList => {
                    tools.push(Arc::new(HostExecListTool::new(
                        manager.clone(),
                        catalog.clone(),
                    )));
                }
                ProductToolName::ManifestProposePatch => {
                    tools.push(Arc::new(ManifestPatchProposalTool::new(
                        proposal_store.clone(),
                        catalog.clone(),
                    )));
                }
            }
        }
        tools
    }
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

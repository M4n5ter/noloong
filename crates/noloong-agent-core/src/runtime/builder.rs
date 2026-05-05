use super::{AgentRuntime, ContextCompactionRuntime};
use crate::phase::{PHASE_CONTEXT_COMPACT, PHASE_MODEL_REQUEST_PREPARE, PHASE_TURN_DECISION};
use crate::{
    AgentCoreError, CompactionSummarizer, ContextCompactionConfig, ContextCompactor,
    ContextProvider, EventStore, HeuristicTokenEstimator, InMemoryEventStore, ModelProvider,
    PhaseHook, PhaseNode, Result, StandardPhase, StdioExtension, StdioExtensionConfig,
    SummaryContextCompactor, TokenEstimator, ToolCallHook, ToolExecutionMode, ToolProvider,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, atomic::AtomicU64},
};

enum ContextCompactionRegistration {
    Direct {
        config: ContextCompactionConfig,
        compactor: Arc<dyn ContextCompactor>,
        estimator: Arc<dyn TokenEstimator>,
    },
    SummarizerId {
        config: ContextCompactionConfig,
        summarizer_id: String,
        estimator: Arc<dyn TokenEstimator>,
    },
    CompactorId {
        config: ContextCompactionConfig,
        compactor_id: String,
        estimator: Arc<dyn TokenEstimator>,
    },
}

impl AgentRuntime {
    pub fn builder() -> AgentRuntimeBuilder {
        AgentRuntimeBuilder::default()
    }
}

pub struct AgentRuntimeBuilder {
    event_store: Arc<dyn EventStore>,
    phases: Vec<Arc<dyn PhaseNode>>,
    model_providers: BTreeMap<String, Arc<dyn ModelProvider>>,
    default_model_provider: Option<String>,
    tools: BTreeMap<String, Arc<dyn ToolProvider>>,
    tool_execution_mode: ToolExecutionMode,
    tool_hooks: Vec<Arc<dyn ToolCallHook>>,
    phase_hooks: Vec<Arc<dyn PhaseHook>>,
    context_providers: Vec<Arc<dyn ContextProvider>>,
    compaction_summarizers: BTreeMap<String, Arc<dyn CompactionSummarizer>>,
    context_compactors: BTreeMap<String, Arc<dyn ContextCompactor>>,
    http_auth_provider_ids: BTreeSet<String>,
    context_compaction: Option<ContextCompactionRegistration>,
    stdio_extensions: Vec<Arc<StdioExtension>>,
    max_turns: u64,
}

impl Default for AgentRuntimeBuilder {
    fn default() -> Self {
        Self {
            event_store: Arc::new(InMemoryEventStore::new()),
            phases: default_phases(),
            model_providers: BTreeMap::new(),
            default_model_provider: None,
            tools: BTreeMap::new(),
            tool_execution_mode: ToolExecutionMode::Parallel,
            tool_hooks: Vec::new(),
            phase_hooks: Vec::new(),
            context_providers: Vec::new(),
            compaction_summarizers: BTreeMap::new(),
            context_compactors: BTreeMap::new(),
            http_auth_provider_ids: BTreeSet::new(),
            context_compaction: None,
            stdio_extensions: Vec::new(),
            max_turns: 8,
        }
    }
}

impl AgentRuntimeBuilder {
    pub fn with_event_store(mut self, event_store: Arc<dyn EventStore>) -> Self {
        self.event_store = event_store;
        self
    }

    pub fn with_model_provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        let id = provider.id().to_string();
        if self.default_model_provider.is_none() {
            self.default_model_provider = Some(id.clone());
        }
        self.model_providers.insert(id, provider);
        self
    }

    pub fn default_model_provider(mut self, id: impl Into<String>) -> Self {
        self.default_model_provider = Some(id.into());
        self
    }

    pub fn default_model_provider_id(&self) -> Option<&str> {
        self.default_model_provider.as_deref()
    }

    pub fn model_provider_ids(&self) -> impl Iterator<Item = &str> {
        self.model_providers.keys().map(String::as_str)
    }

    pub fn model_provider_metadata(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.model_providers
            .iter()
            .map(|(id, provider)| (id.as_str(), provider.model_name()))
    }

    pub fn with_tool(mut self, tool: Arc<dyn ToolProvider>) -> Self {
        self.tools.insert(tool.spec().name.clone(), tool);
        self
    }

    pub fn without_tool(mut self, name: &str) -> Self {
        self.tools.remove(name);
        self
    }

    pub fn with_tool_execution_mode(mut self, mode: ToolExecutionMode) -> Self {
        self.tool_execution_mode = mode;
        self
    }

    pub fn with_tool_hook(mut self, hook: Arc<dyn ToolCallHook>) -> Self {
        self.tool_hooks.push(hook);
        self
    }

    pub fn with_phase_hook(mut self, hook: Arc<dyn PhaseHook>) -> Self {
        self.phase_hooks.push(hook);
        self
    }

    pub fn with_context_provider(mut self, provider: Arc<dyn ContextProvider>) -> Self {
        self.context_providers.push(provider);
        self
    }

    pub fn with_context_compaction(
        self,
        config: ContextCompactionConfig,
        summarizer: Arc<dyn CompactionSummarizer>,
    ) -> Self {
        self.with_context_compaction_estimator(
            config,
            summarizer,
            Arc::new(HeuristicTokenEstimator),
        )
    }

    pub fn with_context_compaction_estimator(
        self,
        config: ContextCompactionConfig,
        summarizer: Arc<dyn CompactionSummarizer>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.with_context_compactor_estimator(
            config,
            Arc::new(SummaryContextCompactor::new(summarizer)),
            estimator,
        )
    }

    pub fn with_context_compactor(
        self,
        config: ContextCompactionConfig,
        compactor: Arc<dyn ContextCompactor>,
    ) -> Self {
        self.with_context_compactor_estimator(config, compactor, Arc::new(HeuristicTokenEstimator))
    }

    pub fn with_context_compactor_estimator(
        mut self,
        config: ContextCompactionConfig,
        compactor: Arc<dyn ContextCompactor>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.context_compaction = Some(ContextCompactionRegistration::Direct {
            config,
            compactor,
            estimator,
        });
        self
    }

    pub fn with_context_compactor_id(
        self,
        config: ContextCompactionConfig,
        compactor_id: impl Into<String>,
    ) -> Self {
        self.with_context_compactor_id_and_estimator(
            config,
            compactor_id,
            Arc::new(HeuristicTokenEstimator),
        )
    }

    pub fn with_context_compactor_id_and_estimator(
        mut self,
        config: ContextCompactionConfig,
        compactor_id: impl Into<String>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.context_compaction = Some(ContextCompactionRegistration::CompactorId {
            config,
            compactor_id: compactor_id.into(),
            estimator,
        });
        self
    }

    pub fn with_context_compaction_summarizer_id(
        self,
        config: ContextCompactionConfig,
        summarizer_id: impl Into<String>,
    ) -> Self {
        self.with_context_compaction_summarizer_id_and_estimator(
            config,
            summarizer_id,
            Arc::new(HeuristicTokenEstimator),
        )
    }

    pub fn with_context_compaction_summarizer_id_and_estimator(
        mut self,
        config: ContextCompactionConfig,
        summarizer_id: impl Into<String>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.context_compaction = Some(ContextCompactionRegistration::SummarizerId {
            config,
            summarizer_id: summarizer_id.into(),
            estimator,
        });
        self
    }

    pub fn replace_phase(mut self, phase_id: &str, phase: Arc<dyn PhaseNode>) -> Self {
        if let Some(existing) = self.phases.iter_mut().find(|node| node.id() == phase_id) {
            *existing = phase;
        } else {
            self.phases.push(phase);
        }
        self
    }

    pub fn insert_phase_after(mut self, after_phase_id: &str, phase: Arc<dyn PhaseNode>) -> Self {
        if let Some(index) = self
            .phases
            .iter()
            .position(|node| node.id() == after_phase_id)
        {
            self.phases.insert(index + 1, phase);
        } else {
            self.phases.push(phase);
        }
        self
    }

    pub fn max_turns(mut self, max_turns: u64) -> Self {
        self.max_turns = max_turns.max(1);
        self
    }

    pub async fn with_stdio_extension(mut self, config: StdioExtensionConfig) -> Result<Self> {
        let extension = Arc::new(StdioExtension::connect(config).await?);
        let capabilities = extension.capabilities().await?;
        self.validate_extension_capabilities(&capabilities)?;
        for capability in capabilities {
            match capability {
                crate::ExtensionCapability::ModelProvider { id } => {
                    let provider = Arc::new(crate::jsonrpc::StdioModelProvider::new(
                        extension.clone(),
                        id.clone(),
                    ));
                    if self.default_model_provider.is_none() {
                        self.default_model_provider = Some(id.clone());
                    }
                    self.model_providers.insert(id, provider);
                }
                crate::ExtensionCapability::Tool { spec } => {
                    self.tools.insert(
                        spec.name.clone(),
                        Arc::new(crate::jsonrpc::StdioToolProvider::new(
                            extension.clone(),
                            spec,
                        )),
                    );
                }
                crate::ExtensionCapability::ContextProvider { id } => {
                    self.context_providers.push(Arc::new(
                        crate::jsonrpc::StdioContextProvider::new(extension.clone(), id),
                    ));
                }
                crate::ExtensionCapability::PhaseNode { id } => {
                    let phase =
                        Arc::new(crate::jsonrpc::StdioPhaseNode::new(extension.clone(), id));
                    insert_before_phase(&mut self.phases, PHASE_TURN_DECISION, phase);
                }
                crate::ExtensionCapability::PhaseHook { id } => {
                    self.phase_hooks
                        .push(Arc::new(crate::jsonrpc::StdioPhaseHook::new(
                            extension.clone(),
                            id,
                        )));
                }
                crate::ExtensionCapability::ToolCallHook { id } => {
                    self.tool_hooks
                        .push(Arc::new(crate::jsonrpc::StdioToolCallHook::new(
                            extension.clone(),
                            id,
                        )));
                }
                crate::ExtensionCapability::CompactionSummarizer { id } => {
                    self.compaction_summarizers.insert(
                        id.clone(),
                        Arc::new(crate::jsonrpc::StdioCompactionSummarizer::new(
                            extension.clone(),
                            id,
                        )),
                    );
                }
                crate::ExtensionCapability::ContextCompactor { id } => {
                    self.context_compactors.insert(
                        id.clone(),
                        Arc::new(crate::jsonrpc::StdioContextCompactor::new(
                            extension.clone(),
                            id,
                        )),
                    );
                }
                crate::ExtensionCapability::HttpAuthProvider { id } => {
                    self.http_auth_provider_ids.insert(id);
                }
            }
        }
        self.stdio_extensions.push(extension);
        Ok(self)
    }

    fn validate_extension_capabilities(
        &self,
        capabilities: &[crate::ExtensionCapability],
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        for capability in capabilities {
            match capability {
                crate::ExtensionCapability::ModelProvider { id } => ensure_unique_capability(
                    &mut seen,
                    "model provider",
                    id,
                    self.model_providers.contains_key(id),
                )?,
                crate::ExtensionCapability::Tool { spec } => ensure_unique_capability(
                    &mut seen,
                    "tool",
                    &spec.name,
                    self.tools.contains_key(&spec.name),
                )?,
                crate::ExtensionCapability::ContextProvider { id } => ensure_unique_capability(
                    &mut seen,
                    "context provider",
                    id,
                    self.context_providers
                        .iter()
                        .any(|provider| provider.id() == id),
                )?,
                crate::ExtensionCapability::PhaseNode { id } => ensure_unique_capability(
                    &mut seen,
                    "phase",
                    id,
                    self.phases.iter().any(|phase| phase.id() == id),
                )?,
                crate::ExtensionCapability::PhaseHook { id } => ensure_unique_capability(
                    &mut seen,
                    "phase hook",
                    id,
                    self.phase_hooks
                        .iter()
                        .any(|hook| hook.id().is_some_and(|hook_id| hook_id == id.as_str())),
                )?,
                crate::ExtensionCapability::ToolCallHook { id } => ensure_unique_capability(
                    &mut seen,
                    "tool call hook",
                    id,
                    self.tool_hooks
                        .iter()
                        .any(|hook| hook.id().is_some_and(|hook_id| hook_id == id.as_str())),
                )?,
                crate::ExtensionCapability::CompactionSummarizer { id } => {
                    ensure_unique_capability(
                        &mut seen,
                        "compaction summarizer",
                        id,
                        self.compaction_summarizers.contains_key(id),
                    )?
                }
                crate::ExtensionCapability::ContextCompactor { id } => ensure_unique_capability(
                    &mut seen,
                    "context compactor",
                    id,
                    self.context_compactors.contains_key(id),
                )?,
                crate::ExtensionCapability::HttpAuthProvider { id } => ensure_unique_capability(
                    &mut seen,
                    "http auth provider",
                    id,
                    self.http_auth_provider_ids.contains(id),
                )?,
            }
        }
        Ok(())
    }

    pub fn build(self) -> Result<AgentRuntime> {
        let default_model_provider = self.default_model_provider.ok_or_else(|| {
            AgentCoreError::MissingModelProvider("no default model provider registered".into())
        })?;
        if !self.model_providers.contains_key(&default_model_provider) {
            return Err(AgentCoreError::MissingModelProvider(default_model_provider));
        }
        let context_compaction = resolve_context_compaction(
            self.context_compaction,
            &self.compaction_summarizers,
            &self.context_compactors,
        )?;
        let mut phases = self.phases;
        if context_compaction.is_some() {
            ensure_context_compaction_phase(&mut phases);
        }
        Ok(AgentRuntime {
            event_store: self.event_store,
            phases,
            model_providers: self.model_providers,
            default_model_provider,
            tools: self.tools,
            tool_execution_mode: self.tool_execution_mode,
            tool_hooks: self.tool_hooks,
            phase_hooks: self.phase_hooks,
            context_providers: self.context_providers,
            context_compaction,
            _stdio_extensions: self.stdio_extensions,
            max_turns: self.max_turns,
            run_counter: Arc::new(AtomicU64::new(0)),
            event_counter: Arc::new(AtomicU64::new(0)),
        })
    }
}

fn default_phases() -> Vec<Arc<dyn PhaseNode>> {
    vec![
        Arc::new(StandardPhase::InputIngest),
        Arc::new(StandardPhase::ContextPrepare),
        Arc::new(StandardPhase::ModelRequestPrepare),
        Arc::new(StandardPhase::ModelStream),
        Arc::new(StandardPhase::AssistantCommit),
        Arc::new(StandardPhase::ToolCallResolve),
        Arc::new(StandardPhase::ToolExecute),
        Arc::new(StandardPhase::TurnDecision),
    ]
}

fn ensure_context_compaction_phase(phases: &mut Vec<Arc<dyn PhaseNode>>) {
    if phases.iter().any(|node| node.id() == PHASE_CONTEXT_COMPACT) {
        return;
    }
    insert_before_phase(
        phases,
        PHASE_MODEL_REQUEST_PREPARE,
        Arc::new(StandardPhase::ContextCompact),
    );
}

fn insert_before_phase(
    phases: &mut Vec<Arc<dyn PhaseNode>>,
    before_phase_id: &str,
    phase: Arc<dyn PhaseNode>,
) {
    if let Some(index) = phases.iter().position(|node| node.id() == before_phase_id) {
        phases.insert(index, phase);
    } else {
        phases.push(phase);
    }
}

fn ensure_unique_capability<'a>(
    seen: &mut BTreeSet<(&'static str, &'a str)>,
    kind: &'static str,
    id: &'a str,
    exists: bool,
) -> Result<()> {
    if exists || !seen.insert((kind, id)) {
        return Err(duplicate_extension_capability(kind, id));
    }
    Ok(())
}

fn duplicate_extension_capability(kind: &str, id: &str) -> AgentCoreError {
    AgentCoreError::JsonRpc(format!("duplicate extension {kind}: {id}"))
}

fn resolve_context_compaction(
    registration: Option<ContextCompactionRegistration>,
    summarizers: &BTreeMap<String, Arc<dyn CompactionSummarizer>>,
    compactors: &BTreeMap<String, Arc<dyn ContextCompactor>>,
) -> Result<Option<ContextCompactionRuntime>> {
    let Some(registration) = registration else {
        return Ok(None);
    };
    match registration {
        ContextCompactionRegistration::Direct {
            config,
            compactor,
            estimator,
        } => {
            config.validate()?;
            Ok(Some(ContextCompactionRuntime {
                config,
                compactor,
                estimator,
            }))
        }
        ContextCompactionRegistration::SummarizerId {
            config,
            summarizer_id,
            estimator,
        } => {
            config.validate()?;
            let summarizer = summarizers.get(&summarizer_id).cloned().ok_or_else(|| {
                AgentCoreError::Phase(format!("compaction summarizer not found: {summarizer_id}"))
            })?;
            Ok(Some(ContextCompactionRuntime {
                config,
                compactor: Arc::new(SummaryContextCompactor::new(summarizer)),
                estimator,
            }))
        }
        ContextCompactionRegistration::CompactorId {
            config,
            compactor_id,
            estimator,
        } => {
            config.validate()?;
            let compactor = compactors.get(&compactor_id).cloned().ok_or_else(|| {
                AgentCoreError::Phase(format!("context compactor not found: {compactor_id}"))
            })?;
            Ok(Some(ContextCompactionRuntime {
                config,
                compactor,
                estimator,
            }))
        }
    }
}

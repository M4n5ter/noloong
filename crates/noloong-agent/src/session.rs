use crate::{
    AgentManifest, ApplyPatchTool, BuiltInApprovalHook, BuiltInToolName,
    BuiltInToolOutputOverflowHook, Catalog, FileEditManager, FileEditToolPolicy, GoalRecord,
    HostEnvironment, HostProcessCompletion, HostProcessEvent, HostProcessManager,
    HostProcessSubscription, ManifestProposalStore, ToolOutputOverflowConfig, WriteFileTool,
    approval::{ApprovalCache, cache_key_from_approval_resolution},
    plugin::{PluginLoadError, PluginLoadFailurePolicy, PluginLoadWarning},
    system_prompt::{
        BUILT_IN_SYSTEM_PROMPT_HOOK_ID, ResolvedSystemPrompt, SystemPromptModelContext,
    },
    text,
    tools::{
        APPLY_PATCH_TOOL_NAME, GoalController, GoalUpdateTool, HostExecListTool, HostExecReadTool,
        HostExecStartTool, HostExecTerminateTool, HostExecWaitTool, HostExecWriteTool,
        ManifestPatchProposalTool, SubagentController, SubagentListTool, SubagentResultTool,
        SubagentSpawnTool, SubagentWaitTool, WRITE_FILE_TOOL_NAME,
    },
};
use noloong_agent_core::{
    Agent, AgentMessage, AgentRuntime, AgentRuntimeBuilder, BeforeModelRequestHookContext,
    BeforeModelRequestHookResult, CompactionSummarizer, ContentBlock, ContextCompactionConfig,
    ContextCompactor, ContextProvider, EventStore, MessageRole, ModelProvider, PhaseHook,
    PhaseNode, Result, StdioExtensionConfig, TokenEstimator, ToolApprovalRequest, ToolCallHook,
    ToolExecutionMode, ToolPermissionDecision, ToolProvider,
};
use serde_json::{Map, json};
use std::{
    collections::BTreeMap,
    env,
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
    file_edit_manager: FileEditManager,
    proposal_store: ManifestProposalStore,
    subagent_controller: Option<Arc<dyn SubagentController>>,
    goal_controller: Option<Arc<dyn GoalController>>,
    active_goal: Mutex<Option<GoalRecord>>,
    subagent_depth: usize,
    max_subagent_depth: usize,
    tool_output_overflow_config: ToolOutputOverflowConfig,
    approval_cache: ApprovalCache,
    system_prompt_model_context: Mutex<Option<SystemPromptModelContext>>,
}

pub struct AgentSessionBuilder {
    manifest: AgentManifest,
    environment: Option<HostEnvironment>,
    process_manager: Option<HostProcessManager>,
    proposal_store: ManifestProposalStore,
    subagent_controller: Option<Arc<dyn SubagentController>>,
    goal_controller: Option<Arc<dyn GoalController>>,
    active_goal: Option<GoalRecord>,
    subagent_depth: usize,
    max_subagent_depth: usize,
    tool_output_overflow_config: ToolOutputOverflowConfig,
    approval_cache: ApprovalCache,
}

impl Default for AgentSessionBuilder {
    fn default() -> Self {
        Self {
            manifest: AgentManifest::default(),
            environment: None,
            process_manager: None,
            proposal_store: ManifestProposalStore::default(),
            subagent_controller: None,
            goal_controller: None,
            active_goal: None,
            subagent_depth: 0,
            max_subagent_depth: 1,
            tool_output_overflow_config: ToolOutputOverflowConfig::default(),
            approval_cache: ApprovalCache::default(),
        }
    }
}

pub struct AgentSessionRuntimeBuilder {
    core: AgentRuntimeBuilder,
    inner: Arc<AgentSessionInner>,
    manifest: AgentManifest,
    catalog: Catalog,
    model_names_by_id: BTreeMap<String, String>,
    default_model_provider: Option<String>,
    plugin_load_warnings: Vec<PluginLoadWarning>,
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

    pub(crate) fn set_active_goal_for_tools(&self, goal: Option<GoalRecord>) {
        *self
            .inner
            .active_goal
            .lock()
            .expect("agent session active goal lock poisoned") = goal;
    }

    pub fn resolved_system_prompt(&self) -> ResolvedSystemPrompt {
        resolved_system_prompt_from_inner(&self.inner)
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

    pub fn runtime_builder(&self) -> AgentSessionRuntimeBuilder {
        let manifest = self.manifest();
        let catalog = Catalog::new(manifest.locale);
        let mut builder = AgentRuntime::builder()
            .with_phase_hook(Arc::new(BuiltInSystemPromptHook::new(Arc::clone(
                &self.inner,
            ))))
            .with_context_provider(Arc::new(BuiltInHostContextProvider::new(
                self.inner.environment.clone(),
                catalog.clone(),
            )))
            .with_tool_hook(Arc::new(
                BuiltInApprovalHook::new(manifest.approval_policy.clone(), catalog.clone())
                    .with_approval_cache(self.inner.approval_cache.clone()),
            ))
            .with_tool_hook(Arc::new(
                BuiltInToolOutputOverflowHook::new(self.inner.tool_output_overflow_config.clone())
                    .with_catalog(catalog.clone()),
            ));
        for tool in self.tools_for_manifest(&manifest, &catalog) {
            builder = builder.with_tool(tool);
        }
        AgentSessionRuntimeBuilder {
            core: builder,
            inner: Arc::clone(&self.inner),
            manifest,
            catalog,
            model_names_by_id: BTreeMap::new(),
            default_model_provider: None,
            plugin_load_warnings: Vec::new(),
        }
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

    pub fn record_tool_approval_resolution(
        &self,
        approval: &ToolApprovalRequest,
        decision: &ToolPermissionDecision,
    ) -> bool {
        let Some(cache_key) = cache_key_from_approval_resolution(approval, decision) else {
            return false;
        };
        self.inner.approval_cache.insert(cache_key)
    }

    fn tools_for_manifest(
        &self,
        manifest: &AgentManifest,
        catalog: &Catalog,
    ) -> Vec<Arc<dyn ToolProvider>> {
        manifest
            .enabled_tools
            .iter()
            .filter_map(|name| self.tool_for_name(*name, catalog))
            .collect()
    }

    fn tool_for_name(
        &self,
        name: BuiltInToolName,
        catalog: &Catalog,
    ) -> Option<Arc<dyn ToolProvider>> {
        match name {
            BuiltInToolName::HostExecStart => Some(Arc::new(HostExecStartTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            ))),
            BuiltInToolName::HostExecRead => Some(Arc::new(HostExecReadTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            ))),
            BuiltInToolName::HostExecWait => Some(Arc::new(HostExecWaitTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            ))),
            BuiltInToolName::HostExecWrite => Some(Arc::new(HostExecWriteTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            ))),
            BuiltInToolName::HostExecTerminate => Some(Arc::new(HostExecTerminateTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            ))),
            BuiltInToolName::HostExecList => Some(Arc::new(HostExecListTool::new(
                self.inner.process_manager.clone(),
                catalog.clone(),
            ))),
            BuiltInToolName::SubagentSpawn => self.subagent_controller().map(|controller| {
                Arc::new(SubagentSpawnTool::new(controller, catalog.clone()))
                    as Arc<dyn ToolProvider>
            }),
            BuiltInToolName::SubagentWait => self.subagent_controller().map(|controller| {
                Arc::new(SubagentWaitTool::new(controller, catalog.clone()))
                    as Arc<dyn ToolProvider>
            }),
            BuiltInToolName::SubagentResult => self.subagent_controller().map(|controller| {
                Arc::new(SubagentResultTool::new(controller, catalog.clone()))
                    as Arc<dyn ToolProvider>
            }),
            BuiltInToolName::SubagentList => self.subagent_controller().map(|controller| {
                Arc::new(SubagentListTool::new(controller, catalog.clone()))
                    as Arc<dyn ToolProvider>
            }),
            BuiltInToolName::GoalUpdate => self.goal_controller().map(|controller| {
                Arc::new(GoalUpdateTool::new(controller, catalog.clone())) as Arc<dyn ToolProvider>
            }),
            BuiltInToolName::ManifestProposePatch => Some(Arc::new(
                ManifestPatchProposalTool::new(self.inner.proposal_store.clone(), catalog.clone()),
            )),
        }
    }

    fn subagent_controller(&self) -> Option<Arc<dyn SubagentController>> {
        if self.inner.subagent_depth >= self.inner.max_subagent_depth {
            return None;
        }
        self.inner.subagent_controller.clone()
    }

    fn goal_controller(&self) -> Option<Arc<dyn GoalController>> {
        self.inner.goal_controller.clone()
    }
}

struct BuiltInSystemPromptHook {
    inner: Arc<AgentSessionInner>,
}

impl BuiltInSystemPromptHook {
    fn new(inner: Arc<AgentSessionInner>) -> Self {
        Self { inner }
    }
}

impl PhaseHook for BuiltInSystemPromptHook {
    fn id(&self) -> Option<&str> {
        Some(BUILT_IN_SYSTEM_PROMPT_HOOK_ID)
    }

    fn before_model_request<'a>(
        &'a self,
        context: BeforeModelRequestHookContext<'a>,
        cancellation: noloong_agent_core::CancellationToken,
    ) -> noloong_agent_core::BoxFuture<'a, Option<BeforeModelRequestHookResult>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let prompt = resolved_system_prompt_from_inner(&self.inner);
            let mut request = context.request.clone();
            if !goal_update_tool_available(&self.inner) {
                request
                    .tools
                    .retain(|tool| tool.name != BuiltInToolName::GoalUpdate.as_str());
            }
            request.messages.insert(
                0,
                system_prompt_message(&prompt, context.run_id, context.turn_id),
            );
            Ok(Some(BeforeModelRequestHookResult { request }))
        })
    }
}

fn goal_update_tool_available(inner: &AgentSessionInner) -> bool {
    inner
        .active_goal
        .lock()
        .expect("agent session active goal lock poisoned")
        .as_ref()
        .is_some_and(GoalRecord::is_pursuing)
}

fn resolved_system_prompt_from_inner(inner: &AgentSessionInner) -> ResolvedSystemPrompt {
    let (locale, system_prompt) = {
        let manifest = inner
            .manifest
            .lock()
            .expect("agent session manifest lock poisoned");
        (manifest.locale, manifest.system_prompt.clone())
    };
    let model = inner
        .system_prompt_model_context
        .lock()
        .expect("agent session model context lock poisoned")
        .clone();
    crate::system_prompt::resolve_system_prompt(locale, &system_prompt, model.as_ref())
}

fn system_prompt_message(
    prompt: &ResolvedSystemPrompt,
    run_id: &str,
    turn_id: u64,
) -> AgentMessage {
    let mut metadata = Map::new();
    metadata.insert("noloong.kind".into(), json!("system_prompt"));
    metadata.insert("noloong.source".into(), json!(prompt.source.as_str()));
    if let Some(configured_profile) = prompt.configured_profile {
        metadata.insert(
            "noloong.configuredProfile".into(),
            json!(configured_profile.as_str()),
        );
    }
    if let Some(resolved_profile) = prompt.resolved_profile {
        metadata.insert(
            "noloong.resolvedProfile".into(),
            json!(resolved_profile.as_str()),
        );
    }
    metadata.insert(
        "noloong.enabledAdditionIds".into(),
        json!(prompt.enabled_addition_ids.clone()),
    );
    AgentMessage {
        id: format!("noloong-system-prompt-{run_id}-{turn_id}"),
        role: MessageRole::System,
        content: vec![ContentBlock::Text {
            text: prompt.effective_text.clone(),
        }],
        metadata,
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

    pub fn with_subagent_controller(mut self, controller: Arc<dyn SubagentController>) -> Self {
        self.subagent_controller = Some(controller);
        self
    }

    pub fn with_goal_controller(mut self, controller: Arc<dyn GoalController>) -> Self {
        self.goal_controller = Some(controller);
        self
    }

    pub(crate) fn with_active_goal(mut self, goal: Option<GoalRecord>) -> Self {
        self.active_goal = goal;
        self
    }

    pub(crate) fn with_subagent_depth(mut self, depth: usize) -> Self {
        self.subagent_depth = depth;
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
        let file_edit_manager = FileEditManager::new(environment.cwd.clone());
        AgentSession {
            inner: Arc::new(AgentSessionInner {
                manifest: Mutex::new(self.manifest),
                environment,
                process_manager: self.process_manager.unwrap_or_default(),
                file_edit_manager,
                proposal_store: self.proposal_store,
                subagent_controller: self.subagent_controller,
                goal_controller: self.goal_controller,
                active_goal: Mutex::new(self.active_goal),
                subagent_depth: self.subagent_depth,
                max_subagent_depth: self.max_subagent_depth,
                tool_output_overflow_config: self.tool_output_overflow_config,
                approval_cache: self.approval_cache,
                system_prompt_model_context: Mutex::new(None),
            }),
        }
    }
}

impl AgentSessionRuntimeBuilder {
    pub fn with_event_store(mut self, event_store: Arc<dyn EventStore>) -> Self {
        self.core = self.core.with_event_store(event_store);
        self
    }

    pub fn with_model_provider(mut self, provider: Arc<dyn ModelProvider>) -> Self {
        let id = provider.id().to_string();
        let model_name = provider.model_name().unwrap_or(provider.id()).to_string();
        if self.default_model_provider.is_none() {
            self.default_model_provider = Some(id.clone());
        }
        self.model_names_by_id.insert(id, model_name);
        self.core = self.core.with_model_provider(provider);
        self
    }

    pub fn default_model_provider(mut self, id: impl Into<String>) -> Self {
        let id = id.into();
        self.default_model_provider = Some(id.clone());
        self.core = self.core.default_model_provider(id);
        self
    }

    pub fn with_tool(mut self, tool: Arc<dyn ToolProvider>) -> Self {
        self.core = self.core.with_tool(tool);
        self
    }

    pub fn without_tool(mut self, name: &str) -> Self {
        self.core = self.core.without_tool(name);
        self
    }

    pub fn configure_core(
        mut self,
        configure: impl FnOnce(AgentRuntimeBuilder) -> AgentRuntimeBuilder,
    ) -> Self {
        self.core = configure(self.core);
        self.sync_model_provider_metadata_from_core();
        self
    }

    pub fn with_tool_execution_mode(mut self, mode: ToolExecutionMode) -> Self {
        self.core = self.core.with_tool_execution_mode(mode);
        self
    }

    pub fn with_tool_hook(mut self, hook: Arc<dyn ToolCallHook>) -> Self {
        self.core = self.core.with_tool_hook(hook);
        self
    }

    pub fn with_phase_hook(mut self, hook: Arc<dyn PhaseHook>) -> Self {
        self.core = self.core.with_phase_hook(hook);
        self
    }

    pub fn with_context_provider(mut self, provider: Arc<dyn ContextProvider>) -> Self {
        self.core = self.core.with_context_provider(provider);
        self
    }

    pub fn with_context_compaction(
        mut self,
        config: ContextCompactionConfig,
        summarizer: Arc<dyn CompactionSummarizer>,
    ) -> Self {
        self.core = self.core.with_context_compaction(config, summarizer);
        self
    }

    pub fn with_context_compaction_estimator(
        mut self,
        config: ContextCompactionConfig,
        summarizer: Arc<dyn CompactionSummarizer>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.core = self
            .core
            .with_context_compaction_estimator(config, summarizer, estimator);
        self
    }

    pub fn with_context_compactor(
        mut self,
        config: ContextCompactionConfig,
        compactor: Arc<dyn ContextCompactor>,
    ) -> Self {
        self.core = self.core.with_context_compactor(config, compactor);
        self
    }

    pub fn with_context_compactor_estimator(
        mut self,
        config: ContextCompactionConfig,
        compactor: Arc<dyn ContextCompactor>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.core = self
            .core
            .with_context_compactor_estimator(config, compactor, estimator);
        self
    }

    pub fn with_context_compactor_id(
        mut self,
        config: ContextCompactionConfig,
        compactor_id: impl Into<String>,
    ) -> Self {
        self.core = self.core.with_context_compactor_id(config, compactor_id);
        self
    }

    pub fn with_context_compactor_id_and_estimator(
        mut self,
        config: ContextCompactionConfig,
        compactor_id: impl Into<String>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.core =
            self.core
                .with_context_compactor_id_and_estimator(config, compactor_id, estimator);
        self
    }

    pub fn with_context_compaction_summarizer_id(
        mut self,
        config: ContextCompactionConfig,
        summarizer_id: impl Into<String>,
    ) -> Self {
        self.core = self
            .core
            .with_context_compaction_summarizer_id(config, summarizer_id);
        self
    }

    pub fn with_context_compaction_summarizer_id_and_estimator(
        mut self,
        config: ContextCompactionConfig,
        summarizer_id: impl Into<String>,
        estimator: Arc<dyn TokenEstimator>,
    ) -> Self {
        self.core = self
            .core
            .with_context_compaction_summarizer_id_and_estimator(config, summarizer_id, estimator);
        self
    }

    pub fn replace_phase(mut self, phase_id: &str, phase: Arc<dyn PhaseNode>) -> Self {
        self.core = self.core.replace_phase(phase_id, phase);
        self
    }

    pub fn insert_phase_after(mut self, after_phase_id: &str, phase: Arc<dyn PhaseNode>) -> Self {
        self.core = self.core.insert_phase_after(after_phase_id, phase);
        self
    }

    pub fn max_turns(mut self, max_turns: u64) -> Self {
        self.core = self.core.max_turns(max_turns);
        self
    }

    pub async fn with_stdio_extension(mut self, config: StdioExtensionConfig) -> Result<Self> {
        self.core = self.core.with_stdio_extension(config).await?;
        self.sync_model_provider_metadata_from_core();
        Ok(self)
    }

    pub async fn with_manifest_plugins(mut self) -> Result<Self> {
        let plugins = self
            .manifest
            .plugins
            .values()
            .filter(|plugin| plugin.enabled)
            .cloned()
            .collect::<Vec<_>>();
        for plugin in plugins {
            let plugin_id = plugin.plugin_id.clone();
            let on_load_failure = plugin.on_load_failure;
            let config = match plugin.to_stdio_extension_config(|name| env::var(name).ok()) {
                Ok(config) => config,
                Err(error) => match on_load_failure {
                    PluginLoadFailurePolicy::DisableForRun => {
                        self.plugin_load_warnings.push(PluginLoadWarning {
                            plugin_id,
                            message: error.to_string(),
                        });
                        continue;
                    }
                    PluginLoadFailurePolicy::FailRun => {
                        return Err(noloong_agent_core::AgentCoreError::Provider(
                            error.to_string(),
                        ));
                    }
                },
            };
            match self.core.add_stdio_extension(config).await {
                Ok(()) => {
                    self.sync_model_provider_metadata_from_core();
                }
                Err(error) => match on_load_failure {
                    PluginLoadFailurePolicy::DisableForRun => {
                        let error = PluginLoadError::Startup {
                            plugin_id: plugin_id.clone(),
                            message: format!("{}: {error}", plugin.summary()),
                        };
                        self.plugin_load_warnings.push(PluginLoadWarning {
                            plugin_id,
                            message: error.to_string(),
                        });
                    }
                    PluginLoadFailurePolicy::FailRun => {
                        return Err(noloong_agent_core::AgentCoreError::Provider(
                            PluginLoadError::Startup {
                                plugin_id,
                                message: format!("{}: {error}", plugin.summary()),
                            }
                            .to_string(),
                        ));
                    }
                },
            }
        }
        Ok(self)
    }

    pub fn plugin_load_warnings(&self) -> &[PluginLoadWarning] {
        &self.plugin_load_warnings
    }

    pub fn build(mut self) -> Result<AgentRuntime> {
        self.sync_model_provider_metadata_from_core();
        let model_context = self.default_model_context();
        *self
            .inner
            .system_prompt_model_context
            .lock()
            .expect("agent session model context lock poisoned") = model_context;
        self.core = self
            .core
            .without_tool(WRITE_FILE_TOOL_NAME)
            .without_tool(APPLY_PATCH_TOOL_NAME);
        if let Some(tool) = self.selected_file_edit_tool() {
            self.core = self.core.with_tool(tool);
        }
        self.core.build()
    }

    fn selected_file_edit_tool(&self) -> Option<Arc<dyn ToolProvider>> {
        match self.manifest.file_edit_tool_policy {
            FileEditToolPolicy::AutoByModel => self.auto_file_edit_tool(),
            FileEditToolPolicy::ApplyPatch => Some(self.apply_patch_tool()),
            FileEditToolPolicy::WriteFile => Some(self.write_file_tool()),
            FileEditToolPolicy::Disabled => None,
        }
    }

    fn auto_file_edit_tool(&self) -> Option<Arc<dyn ToolProvider>> {
        let model_name = self.default_model_name()?;
        if model_name.to_ascii_lowercase().contains("gpt") {
            Some(self.apply_patch_tool())
        } else {
            Some(self.write_file_tool())
        }
    }

    fn default_model_name(&self) -> Option<&str> {
        let provider_id = self.default_model_provider_id()?;
        self.model_names_by_id
            .get(provider_id)
            .map(String::as_str)
            .or(Some(provider_id))
    }

    fn default_model_context(&self) -> Option<SystemPromptModelContext> {
        let provider_id = self.default_model_provider_id()?;
        let model_name = self
            .model_names_by_id
            .get(provider_id)
            .cloned()
            .unwrap_or_else(|| provider_id.to_owned());
        Some(SystemPromptModelContext {
            provider_id: provider_id.to_owned(),
            model_name,
        })
    }

    fn default_model_provider_id(&self) -> Option<&str> {
        self.default_model_provider
            .as_deref()
            .or_else(|| self.model_names_by_id.keys().next().map(String::as_str))
    }

    fn sync_model_provider_metadata_from_core(&mut self) {
        let providers = self
            .core
            .model_provider_metadata()
            .map(|(id, model_name)| {
                (
                    id.to_owned(),
                    model_name
                        .map(str::to_owned)
                        .unwrap_or_else(|| id.to_owned()),
                )
            })
            .collect::<Vec<_>>();
        for (id, model_name) in providers {
            self.model_names_by_id.entry(id).or_insert(model_name);
        }
        if self.default_model_provider.is_none() {
            self.default_model_provider = self.core.default_model_provider_id().map(str::to_owned);
        }
    }

    fn apply_patch_tool(&self) -> Arc<dyn ToolProvider> {
        Arc::new(ApplyPatchTool::new(
            self.inner.file_edit_manager.clone(),
            self.catalog.clone(),
        ))
    }

    fn write_file_tool(&self) -> Arc<dyn ToolProvider> {
        Arc::new(WriteFileTool::new(
            self.inner.file_edit_manager.clone(),
            self.catalog.clone(),
        ))
    }
}

struct BuiltInHostContextProvider {
    environment: HostEnvironment,
    catalog: Catalog,
}

impl BuiltInHostContextProvider {
    fn new(environment: HostEnvironment, catalog: Catalog) -> Self {
        Self {
            environment,
            catalog,
        }
    }
}

impl noloong_agent_core::ContextProvider for BuiltInHostContextProvider {
    fn id(&self) -> &str {
        "noloong.builtin.host-context"
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

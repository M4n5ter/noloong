pub mod approval;
pub mod host;
pub mod i18n;
pub mod interaction;
pub mod manifest;
#[cfg(feature = "openai")]
pub mod openai;
pub mod plugin;
pub mod process;
pub mod session;
pub mod system_prompt;
pub mod tools;

mod text;

pub use approval::{ApprovalPolicy, ApprovalReviewer, BuiltInApprovalHook};
pub use host::{HostEnvironment, Locale, PathStyle};
pub use i18n::{Catalog, MessageKey};
pub use manifest::{
    AgentManifest, AgentSystemPrompt, BuiltInSystemPromptProfile, BuiltInToolName,
    FileEditToolPolicy, ManifestPatch, ManifestPatchProposal, ManifestProposalStore,
    SystemPromptAddition, SystemPromptSource,
};
pub use plugin::{
    AgentPluginDeclaration, PluginEnvSource, PluginLoadError, PluginLoadFailurePolicy,
    PluginLoadWarning, PluginTransport, StdioPluginTransport,
};
pub use process::{
    HostProcessCompletion, HostProcessEvent, HostProcessManager, HostProcessSubscription, JobId,
    JobSnapshot, JobStatus, OutputChunk, ProcessError, ProcessOutput, ProcessOutputStream,
    ReadOutputRequest, StartCommandRequest, WaitOutcome,
};
pub use session::{
    AgentSession, AgentSessionBuilder, AgentSessionRuntimeBuilder, BackgroundCompletionConfig,
    BackgroundCompletionSteering, DEFAULT_BACKGROUND_COMPLETION_PREVIEW_BYTES,
};
pub use system_prompt::{
    BUILT_IN_SYSTEM_PROMPT_HOOK_ID, ResolvedSystemPrompt, SystemPromptModelContext,
    built_in_system_prompt, built_in_system_prompt_for_profile,
};
pub use tools::{
    APPLY_PATCH_TOOL_NAME, ApplyPatchTool, BuiltInToolOutputOverflowHook,
    DEFAULT_MAX_INLINE_TOOL_OUTPUT_BYTES, DEFAULT_TOOL_OUTPUT_PREVIEW_EDGE_BYTES,
    FILE_EDIT_PERMISSION_CAPABILITY, FileEditManager, HostExecListTool, HostExecReadTool,
    HostExecStartTool, HostExecTerminateTool, HostExecWaitTool, HostExecWriteTool,
    ManifestPatchProposalTool, SubagentController, SubagentFinalOutput, SubagentListTool,
    SubagentResult, SubagentResultTool, SubagentSpawnRequest, SubagentSpawnTool, SubagentSummary,
    SubagentWaitOutcome, SubagentWaitTool, ToolOutputOverflowConfig, WRITE_FILE_TOOL_NAME,
    WriteFileTool,
};

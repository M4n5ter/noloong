pub mod approval;
pub mod host;
pub mod i18n;
pub mod manifest;
pub mod process;
pub mod session;
pub mod tools;

mod text;

pub use approval::{ApprovalPolicy, ApprovalReviewer, BuiltInApprovalHook};
pub use host::{HostEnvironment, Locale, PathStyle};
pub use i18n::{Catalog, MessageKey};
pub use manifest::{
    AgentManifest, BuiltInToolName, ManifestPatch, ManifestPatchProposal, ManifestProposalStore,
};
pub use process::{
    HostProcessCompletion, HostProcessEvent, HostProcessManager, HostProcessSubscription, JobId,
    JobSnapshot, JobStatus, OutputChunk, ProcessError, ProcessOutput, ProcessOutputStream,
    ReadOutputRequest, StartCommandRequest, WaitOutcome,
};
pub use session::{
    AgentSession, AgentSessionBuilder, BackgroundCompletionConfig, BackgroundCompletionSteering,
    DEFAULT_BACKGROUND_COMPLETION_PREVIEW_BYTES,
};
pub use tools::{
    BuiltInToolOutputOverflowHook, DEFAULT_MAX_INLINE_TOOL_OUTPUT_BYTES,
    DEFAULT_TOOL_OUTPUT_PREVIEW_EDGE_BYTES, HostExecListTool, HostExecReadTool, HostExecStartTool,
    HostExecTerminateTool, HostExecWaitTool, HostExecWriteTool, ManifestPatchProposalTool,
    ToolOutputOverflowConfig,
};

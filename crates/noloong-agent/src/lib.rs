pub mod approval;
pub mod host;
pub mod i18n;
pub mod manifest;
pub mod process;
pub mod session;
pub mod tools;

pub use approval::{ApprovalPolicy, ApprovalReviewer, ProductApprovalHook};
pub use host::{HostEnvironment, Locale, PathStyle};
pub use i18n::{Catalog, MessageKey};
pub use manifest::{
    AgentManifest, ManifestPatch, ManifestPatchProposal, ManifestProposalStore, ProductToolName,
};
pub use process::{
    HostProcessManager, JobId, JobSnapshot, JobStatus, OutputChunk, ProcessError, ProcessOutput,
    ProcessOutputStream, ReadOutputRequest, StartCommandRequest, WaitOutcome,
};
pub use session::{AgentSession, AgentSessionBuilder};
pub use tools::{
    HostExecListTool, HostExecReadTool, HostExecStartTool, HostExecTerminateTool, HostExecWaitTool,
    HostExecWriteTool, ManifestPatchProposalTool,
};

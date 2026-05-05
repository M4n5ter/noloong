use super::{InteractionFuture, InteractionProfileDescriptor};
use crate::{AgentManifest, AgentSession};
use noloong_agent_core::AgentRuntime;

pub trait AgentRuntimeProfile: Send + Sync {
    fn descriptor(&self) -> InteractionProfileDescriptor;

    fn build_runtime<'a>(
        &'a self,
        session: &'a AgentSession,
        manifest: &'a AgentManifest,
    ) -> InteractionFuture<'a, AgentRuntime>;
}

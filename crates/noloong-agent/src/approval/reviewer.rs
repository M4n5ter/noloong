use noloong_agent_core::{
    BeforeToolCallContext, BoxFuture, CancellationToken, ToolPermissionDecision,
};

pub trait ApprovalReviewer: Send + Sync {
    fn review_tool_call<'a>(
        &'a self,
        context: BeforeToolCallContext,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolPermissionDecision>;
}

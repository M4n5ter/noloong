use noloong_agent::{
    Catalog, GoalController, GoalRecord, GoalStatus, GoalUpdateRequest, GoalUpdateTool, Locale,
};
use noloong_agent_core::{
    AgentCoreError, AgentState, BoxFuture, CancellationToken, ToolProvider, ToolRequest,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn goal_update_tool_updates_goal_with_run_context() {
    let controller = Arc::new(FakeGoalController::default());
    let tool = GoalUpdateTool::new(controller.clone(), Catalog::new(Locale::En));

    let output = tool
        .execute_tool(
            request(json!({
                "status": "achieved",
                "summary": " done ",
                "evidence": " final output "
            })),
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(output.details["status"], "achieved");
    let calls = controller.calls.lock().unwrap();
    assert_eq!(calls[0].0.status, GoalStatus::Achieved);
    assert_eq!(calls[0].0.summary.as_deref(), Some("done"));
    assert_eq!(calls[0].0.evidence.as_deref(), Some("final output"));
    assert_eq!(calls[0].1, "run-test");
    assert_eq!(calls[0].2, 7);
}

#[tokio::test]
async fn goal_update_tool_rejects_non_update_status() {
    let tool = GoalUpdateTool::new(
        Arc::new(FakeGoalController::default()),
        Catalog::new(Locale::En),
    );

    let error = tool
        .execute_tool(
            request(json!({
                "status": "paused"
            })),
            CancellationToken::new(),
        )
        .await
        .expect_err("paused is not a valid tool update status");

    assert!(error.to_string().contains("goal update status must be"));
}

#[tokio::test]
async fn goal_update_tool_surfaces_missing_goal() {
    let tool = GoalUpdateTool::new(
        Arc::new(FakeGoalController {
            fail_missing_goal: true,
            ..FakeGoalController::default()
        }),
        Catalog::new(Locale::En),
    );

    let error = tool
        .execute_tool(
            request(json!({"status": "achieved"})),
            CancellationToken::new(),
        )
        .await
        .expect_err("missing active goal should fail");

    assert!(error.to_string().contains("goal not found"));
}

#[derive(Default)]
struct FakeGoalController {
    calls: Mutex<Vec<(GoalUpdateRequest, String, u64)>>,
    fail_missing_goal: bool,
}

impl GoalController for FakeGoalController {
    fn update_goal<'a>(
        &'a self,
        request: GoalUpdateRequest,
        run_id: String,
        turn_id: u64,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, GoalRecord> {
        Box::pin(async move {
            if self.fail_missing_goal {
                return Err(AgentCoreError::Provider("goal not found".into()));
            }
            self.calls
                .lock()
                .unwrap()
                .push((request.clone(), run_id.clone(), turn_id));
            let mut goal = GoalRecord::new("session-1", "finish");
            goal.status = request.status;
            Ok(goal)
        })
    }
}

fn request(arguments: serde_json::Value) -> ToolRequest {
    ToolRequest {
        run_id: "run-test".into(),
        turn_id: 7,
        tool_call_id: "tool-call-test".into(),
        tool_name: "agent.goal.update".into(),
        arguments,
        state: AgentState::default(),
    }
}

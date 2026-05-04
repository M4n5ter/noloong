pub mod support;

use noloong_agent_core::*;
use serde_json::json;
use support::core::*;
#[test]
fn permission_events_serde_round_trip() -> Result<()> {
    let requirement = ToolPermissionRequirement {
        capability: "test.lookup".into(),
        description: Some("Allows lookup calls".into()),
        metadata: json!({ "scope": "test" }),
    };
    let requested = AgentEvent {
        sequence: 1,
        run_id: "run-1".into(),
        turn_id: Some(1),
        phase: Some("tool.execute".into()),
        kind: AgentEventKind::ToolPermissionRequested {
            tool_call: ToolCall {
                id: "call-1".into(),
                name: "lookup".into(),
                arguments: json!({ "query": "rust" }),
            },
            permissions: vec![requirement],
        },
    };
    let decided = AgentEvent {
        sequence: 2,
        run_id: "run-1".into(),
        turn_id: Some(1),
        phase: Some("tool.execute".into()),
        kind: AgentEventKind::ToolPermissionDecided {
            tool_call_id: "call-1".into(),
            tool_name: "lookup".into(),
            hook_id: Some("policy-hook".into()),
            decision: ToolPermissionDecision {
                outcome: ToolPermissionOutcome::Allow,
                reason: Some("policy matched".into()),
                approver: Some("test".into()),
                metadata: json!({ "policy": "unit" }),
            },
        },
    };

    assert_eq!(
        serde_json::from_value::<AgentEvent>(serde_json::to_value(&requested)?)?,
        requested
    );
    assert_eq!(
        serde_json::from_value::<AgentEvent>(serde_json::to_value(&decided)?)?,
        decided
    );
    Ok(())
}

#[tokio::test]
async fn event_log_replays_to_report_state() -> Result<()> {
    let runtime = native_runtime().build()?;

    let report = runtime.run("hello").await?;
    let replayed = reduce_events(&report.events)?;

    assert_eq!(report.state, replayed);
    assert_eq!(report.state.context.get("native"), Some(&json!("context")));
    assert_eq!(report.state.completed_turns, 2);
    assert_eq!(report.state.messages.len(), 4);
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolExecutionCompleted { tool_call_id, .. }
                if tool_call_id == "call-1"
        )
    }));
    Ok(())
}

pub mod support;

use noloong_agent_core::*;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use support::core::*;
use tokio::sync::{Barrier, Mutex};
use tokio::time::{Duration, timeout};
#[tokio::test]
async fn tool_failure_becomes_auditable_tool_result() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(FailingTool("slow")))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .max_turns(1)
        .build()?;

    let report = runtime.run("tools").await?;

    assert!(matches!(report.state.status, RunStatus::Completed));
    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult {
                    tool_name,
                    is_error: true,
                    ..
                } if tool_name == "slow"
            )
        })
    }));
    assert_eq!(reduce_events(&report.events)?, report.state);
    Ok(())
}

#[tokio::test]
async fn parallel_tools_emit_completion_order_but_commit_source_order() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new(
            "slow",
            Duration::from_millis(50),
        )))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .max_turns(1)
        .build()?;
    let completed = Arc::new(Mutex::new(Vec::new()));
    let completed_events = Arc::clone(&completed);

    let report = runtime
        .run_with_events("tools", move |event| {
            let completed_events = Arc::clone(&completed_events);
            async move {
                if let AgentEventKind::ToolExecutionCompleted {
                    tool_call_id,
                    output: _,
                } = event.kind
                {
                    completed_events.lock().await.push(tool_call_id);
                }
                Ok(())
            }
        })
        .await?;

    assert_eq!(
        completed.lock().await.as_slice(),
        ["fast-call", "slow-call"]
    );
    let committed_tool_names = report
        .state
        .messages
        .iter()
        .filter_map(|message| match message.content.first() {
            Some(ContentBlock::ToolResult { tool_name, .. }) => Some(tool_name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(committed_tool_names, ["slow", "fast"]);
    Ok(())
}

#[tokio::test]
async fn parallel_tool_preflight_runs_before_hooks_concurrently() -> Result<()> {
    let barrier = Arc::new(Barrier::new(2));
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new("slow", Duration::from_millis(0))))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .with_tool_hook(Arc::new(BarrierAllowToolHook {
            barrier: Arc::clone(&barrier),
        }))
        .max_turns(1)
        .build()?;

    timeout(Duration::from_millis(500), runtime.run("tools"))
        .await
        .map_err(|_| AgentCoreError::Phase("parallel tool preflight timed out".into()))??;

    Ok(())
}

#[tokio::test]
async fn sequential_tools_emit_source_order() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new(
            "slow",
            Duration::from_millis(20),
        )))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .with_tool_execution_mode(ToolExecutionMode::Sequential)
        .max_turns(1)
        .build()?;
    let completed = Arc::new(Mutex::new(Vec::new()));
    let completed_events = Arc::clone(&completed);

    runtime
        .run_with_events("tools", move |event| {
            let completed_events = Arc::clone(&completed_events);
            async move {
                if let AgentEventKind::ToolExecutionCompleted { tool_call_id, .. } = event.kind {
                    completed_events.lock().await.push(tool_call_id);
                }
                Ok(())
            }
        })
        .await?;

    assert_eq!(
        completed.lock().await.as_slice(),
        ["slow-call", "fast-call"]
    );
    Ok(())
}

#[tokio::test]
async fn per_tool_execution_mode_can_force_sequential() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new_with_mode(
            "slow",
            Duration::from_millis(20),
            Some(ToolExecutionMode::Sequential),
        )))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .max_turns(1)
        .build()?;
    let completed = Arc::new(Mutex::new(Vec::new()));
    let completed_events = Arc::clone(&completed);

    runtime
        .run_with_events("tools", move |event| {
            let completed_events = Arc::clone(&completed_events);
            async move {
                if let AgentEventKind::ToolExecutionCompleted { tool_call_id, .. } = event.kind {
                    completed_events.lock().await.push(tool_call_id);
                }
                Ok(())
            }
        })
        .await?;

    assert_eq!(
        completed.lock().await.as_slice(),
        ["slow-call", "fast-call"]
    );
    Ok(())
}

#[tokio::test]
async fn tool_hooks_can_block_and_rewrite_results() -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(DelayedTool::new("slow", Duration::from_millis(0))))
        .with_tool(Arc::new(DelayedTool::new("fast", Duration::from_millis(0))))
        .with_tool_hook(Arc::new(TestToolHook))
        .max_turns(1)
        .build()?;

    let report = runtime.run("tools").await?;
    let tool_results = report
        .state
        .messages
        .iter()
        .filter_map(|message| match message.content.first() {
            Some(ContentBlock::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            }) => Some((tool_name.clone(), content.clone(), *is_error)),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(tool_results.len(), 2);
    assert!(
        tool_results
            .iter()
            .any(|(name, _, is_error)| { name == "slow" && *is_error })
    );
    assert!(tool_results.iter().any(|(name, content, is_error)| {
        name == "fast"
            && !*is_error
            && matches!(content.first(), Some(ContentBlock::Text { text }) if text == "rewritten")
    }));
    Ok(())
}

#[tokio::test]
async fn tool_permission_denial_is_audited_and_skips_provider() -> Result<()> {
    let slow_calls = Arc::new(AtomicU64::new(0));
    let fast_calls = Arc::new(AtomicU64::new(0));
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(TwoToolModel))
        .with_tool(Arc::new(PermissionedCountingTool::new(
            "slow",
            Arc::clone(&slow_calls),
        )))
        .with_tool(Arc::new(PermissionedCountingTool::new(
            "fast",
            Arc::clone(&fast_calls),
        )))
        .with_tool_hook(Arc::new(TestToolHook))
        .max_turns(1)
        .build()?;

    let report = runtime.run("tools").await?;

    assert_eq!(slow_calls.load(Ordering::SeqCst), 0);
    assert_eq!(fast_calls.load(Ordering::SeqCst), 1);
    assert_eq!(reduce_events(&report.events)?, report.state);
    assert!(matches!(report.state.status, RunStatus::Completed));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolPermissionRequested { tool_call, permissions }
                if tool_call.name == "slow"
                    && permissions.iter().any(|permission| permission.capability == "test.slow")
        )
    }));
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolPermissionDecided {
                tool_call_id,
                hook_id,
                decision,
                ..
            } if tool_call_id == "slow-call"
                && hook_id.as_deref() == Some("test-tool-hook")
                && decision.outcome == ToolPermissionOutcome::Deny
                && decision.metadata.get("source").and_then(serde_json::Value::as_str) == Some("test")
        )
    }));
    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult {
                    tool_name,
                    content,
                    is_error,
                    ..
                } if tool_name == "slow"
                    && *is_error
                    && matches!(
                        content.first(),
                        Some(ContentBlock::Text { text }) if text == "blocked by test hook"
                    )
            )
        })
    }));
    Ok(())
}

#[tokio::test]
async fn tool_permission_allow_decision_is_audited_and_executes_provider() -> Result<()> {
    let fast_calls = Arc::new(AtomicU64::new(0));
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(FastToolModel))
        .with_tool(Arc::new(PermissionedCountingTool::new(
            "fast",
            Arc::clone(&fast_calls),
        )))
        .with_tool_hook(Arc::new(AllowToolHook))
        .max_turns(1)
        .build()?;

    let report = runtime.run("tools").await?;

    assert_eq!(fast_calls.load(Ordering::SeqCst), 1);
    assert!(report.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolPermissionDecided {
                tool_call_id,
                hook_id,
                decision,
                ..
            } if tool_call_id == "fast-call"
                && hook_id.as_deref() == Some("allow-tool-hook")
                && decision.outcome == ToolPermissionOutcome::Allow
        )
    }));
    assert!(report.state.messages.iter().any(|message| {
        message.content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::ToolResult {
                    tool_name,
                    content,
                    is_error,
                    ..
                } if tool_name == "fast"
                    && !*is_error
                    && matches!(
                        content.first(),
                        Some(ContentBlock::Text { text }) if text == "fast"
                    )
            )
        })
    }));
    Ok(())
}

#[tokio::test]
async fn tool_approval_pauses_and_crash_recovers_on_resume() -> Result<()> {
    let store: Arc<dyn EventStore> = Arc::new(InMemoryEventStore::new());
    let fast_calls = Arc::new(AtomicU64::new(0));
    let runtime = approval_runtime(Arc::clone(&store), Arc::clone(&fast_calls), None)?;

    let paused = runtime.run("tools").await?;

    assert_eq!(fast_calls.load(Ordering::SeqCst), 0);
    assert!(matches!(paused.state.status, RunStatus::Paused));
    assert_eq!(paused.state.pending_tool_approvals.len(), 1);
    assert!(paused.events.iter().any(|event| {
        matches!(&event.kind, AgentEventKind::ToolApprovalRequested { approval }
            if approval.tool_call.id == "fast-call"
                && approval.hook_id.as_deref() == Some("approval-tool-hook"))
    }));
    assert!(
        paused
            .events
            .iter()
            .any(|event| { matches!(&event.kind, AgentEventKind::RunPaused { .. }) })
    );
    assert!(!paused.events.iter().any(|event| {
        matches!(&event.kind, AgentEventKind::ToolExecutionStarted { tool_call_id, .. }
            if tool_call_id == "fast-call")
    }));

    let approval_id = paused
        .state
        .pending_tool_approvals
        .keys()
        .next()
        .expect("approval should be pending")
        .clone();
    let restarted_runtime = approval_runtime(Arc::clone(&store), Arc::clone(&fast_calls), None)?;
    let resumed = restarted_runtime
        .resume_tool_approvals(
            &paused.run_id,
            vec![ToolApprovalResolution {
                approval_id: approval_id.clone(),
                decision: ToolPermissionDecision {
                    outcome: ToolPermissionOutcome::Allow,
                    reason: Some("approved by test".into()),
                    approver: Some("human".into()),
                    metadata: json!({ "ticket": "T-1" }),
                },
            }],
            None,
            CancellationToken::new(),
        )
        .await?;

    assert_eq!(resumed.run_id, paused.run_id);
    assert_eq!(fast_calls.load(Ordering::SeqCst), 1);
    assert!(matches!(resumed.state.status, RunStatus::Completed));
    assert!(resumed.state.pending_tool_approvals.is_empty());
    assert_eq!(reduce_events(&resumed.events)?, resumed.state);
    assert!(
        resumed
            .events
            .windows(2)
            .all(|window| { window[0].sequence < window[1].sequence })
    );
    assert!(resumed.events.iter().any(|event| {
        matches!(&event.kind, AgentEventKind::ToolApprovalResolved {
            approval_id: event_approval_id,
            decision,
        } if event_approval_id == &approval_id
            && decision.outcome == ToolPermissionOutcome::Allow)
    }));
    assert!(
        resumed
            .events
            .iter()
            .any(|event| { matches!(&event.kind, AgentEventKind::RunResumed { .. }) })
    );
    assert!(resumed.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolPermissionDecided {
                tool_call_id,
                hook_id,
                decision,
                ..
            } if tool_call_id == "fast-call"
                && hook_id.as_deref() == Some("approval-tool-hook")
                && decision.outcome == ToolPermissionOutcome::Allow
                && decision.approver.as_deref() == Some("human")
        )
    }));
    assert!(resumed.events.iter().any(|event| {
        matches!(&event.kind, AgentEventKind::ToolExecutionCompleted { tool_call_id, output }
            if tool_call_id == "fast-call" && !output.is_error)
    }));
    Ok(())
}

#[tokio::test]
async fn expired_tool_approval_denies_and_skips_provider() -> Result<()> {
    let store: Arc<dyn EventStore> = Arc::new(InMemoryEventStore::new());
    let fast_calls = Arc::new(AtomicU64::new(0));
    let runtime = approval_runtime(Arc::clone(&store), Arc::clone(&fast_calls), Some(0))?;
    let paused = runtime.run("tools").await?;

    assert!(matches!(paused.state.status, RunStatus::Paused));
    let restarted_runtime = approval_runtime(Arc::clone(&store), Arc::clone(&fast_calls), Some(0))?;
    let resumed = restarted_runtime
        .resume_tool_approvals(&paused.run_id, Vec::new(), None, CancellationToken::new())
        .await?;

    assert_eq!(fast_calls.load(Ordering::SeqCst), 0);
    assert!(matches!(resumed.state.status, RunStatus::Completed));
    assert!(resumed.state.pending_tool_approvals.is_empty());
    assert!(resumed.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolApprovalExpired {
                decision,
                ..
            } if decision.outcome == ToolPermissionOutcome::Deny
                && decision.reason.as_deref() == Some("tool approval timed out")
        )
    }));
    assert!(resumed.events.iter().any(|event| {
        matches!(
            &event.kind,
            AgentEventKind::ToolPermissionDecided {
                tool_call_id,
                hook_id,
                decision,
                ..
            } if tool_call_id == "fast-call"
                && hook_id.as_deref() == Some("approval-tool-hook")
                && decision.outcome == ToolPermissionOutcome::Deny
                && decision.metadata.get("timeout").and_then(serde_json::Value::as_bool)
                    == Some(true)
        )
    }));
    assert!(resumed.events.iter().any(|event| {
        matches!(&event.kind, AgentEventKind::ToolExecutionCompleted { tool_call_id, output }
            if tool_call_id == "fast-call" && output.is_error)
    }));
    Ok(())
}

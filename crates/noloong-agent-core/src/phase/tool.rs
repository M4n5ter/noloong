use super::{PHASE_TOOL_EXECUTE, PhaseContext, PhaseOutput};
use crate::runtime::ToolRuntimeHandles;
use crate::{
    AfterToolCallContext, AgentEffect, AgentMessage, AgentRuntime, AgentState,
    BeforeToolCallContext, BeforeToolCallResult, ContentBlock, Result, RunPauseReason,
    ToolApprovalContinuation, ToolApprovalPreflight, ToolApprovalPreflightStatus,
    ToolApprovalRequest, ToolApprovalResolution, ToolCall, ToolExecutionMode, ToolOutput,
    ToolPermissionAudit, ToolPermissionDecision, ToolPermissionDecisionRecord,
    ToolPermissionOutcome, providers::CancellationToken,
};
use serde_json::json;
use std::collections::BTreeMap;
use tokio::task::JoinSet;

pub(super) async fn tool_execute(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let mut output = PhaseOutput::from_scratch(context.scratch.clone());
    let tool_calls = output.scratch.tool_calls.clone();
    if tool_calls.is_empty() {
        return Ok(output);
    }

    let tool_execution_mode = tool_execution_mode_for_calls(&context, &tool_calls);
    let handles = context.runtime.tool_handles();
    let (preflights, approval_requests) = prepare_tool_preflights(
        handles.clone(),
        context.run_id,
        context.turn_id,
        &context.state,
        &tool_calls,
        tool_execution_mode,
        context.cancellation.clone(),
    )
    .await?;
    if !approval_requests.is_empty() {
        set_tool_approval_pause(
            &mut output,
            context.run_id,
            context.turn_id,
            tool_execution_mode,
            preflights,
            approval_requests,
        );
        return Ok(output);
    }

    let source_order_outputs = if tool_execution_mode == ToolExecutionMode::Sequential {
        execute_prepared_tools_sequential(&context, preflights, &mut output).await?
    } else {
        execute_prepared_tools_parallel(&context, preflights, &mut output).await?
    };

    append_tool_execution_effects(&mut output, context.run_id, source_order_outputs);
    Ok(output)
}

pub(crate) async fn resume_tool_approval_continuation(
    runtime: &AgentRuntime,
    continuation: ToolApprovalContinuation,
    state: AgentState,
    resolutions: Vec<ToolApprovalResolution>,
    cancellation: CancellationToken,
) -> Result<PhaseOutput> {
    if continuation.phase != PHASE_TOOL_EXECUTE {
        return Err(crate::AgentCoreError::Phase(format!(
            "cannot resume approval continuation for phase {}",
            continuation.phase
        )));
    }

    let preflight_context = ToolPreflightContext {
        handles: runtime.tool_handles(),
        run_id: continuation.run_id.clone(),
        turn_id: continuation.turn_id,
        state: state.clone(),
        cancellation: cancellation.clone(),
    };
    let mut resolution_by_id = resolutions
        .into_iter()
        .map(|resolution| (resolution.approval_id.clone(), resolution))
        .collect::<BTreeMap<_, _>>();
    let mut preflights = Vec::with_capacity(continuation.preflights.len());
    let mut approval_requests = Vec::new();

    for preflight in continuation.preflights {
        let ToolApprovalPreflight {
            tool_call,
            mut permission_audit,
            status,
        } = preflight;
        match status {
            ToolApprovalPreflightStatus::Pending {
                approval_id,
                hook_index,
                hook_id,
            } => {
                let resolution = resolution_by_id.remove(&approval_id).ok_or_else(|| {
                    crate::AgentCoreError::Phase(format!(
                        "missing tool approval resolution for {approval_id}"
                    ))
                })?;
                permission_audit
                    .decisions
                    .push(ToolPermissionDecisionRecord {
                        hook_id,
                        decision: resolution.decision.clone(),
                    });
                if matches!(resolution.decision.outcome, ToolPermissionOutcome::Deny) {
                    preflights.push(ToolApprovalPreflight {
                        tool_call,
                        permission_audit,
                        status: ToolApprovalPreflightStatus::Denied {
                            decision: resolution.decision,
                        },
                    });
                    continue;
                }

                let (prepared, approval_request) = prepare_one_tool_call(
                    &preflight_context,
                    tool_call,
                    hook_index + 1,
                    Some(permission_audit),
                )
                .await?;
                if let Some(approval_request) = approval_request {
                    approval_requests.push(approval_request);
                }
                preflights.push(prepared);
            }
            status => {
                preflights.push(ToolApprovalPreflight {
                    tool_call,
                    permission_audit,
                    status,
                });
            }
        }
    }

    if !resolution_by_id.is_empty() {
        let approval_ids = resolution_by_id.keys().cloned().collect::<Vec<_>>();
        return Err(crate::AgentCoreError::Phase(format!(
            "unknown tool approval resolution ids: {}",
            approval_ids.join(", ")
        )));
    }

    let mut output = PhaseOutput::from_scratch(continuation.scratch.clone());
    if !approval_requests.is_empty() {
        set_tool_approval_pause(
            &mut output,
            &continuation.run_id,
            continuation.turn_id,
            continuation.tool_execution_mode,
            preflights,
            approval_requests,
        );
        return Ok(output);
    }

    let context = PhaseContext {
        runtime,
        run_id: &continuation.run_id,
        turn_id: continuation.turn_id,
        state,
        scratch: continuation.scratch,
        cancellation,
        model_stream_sink: None,
    };
    let source_order_outputs = if continuation.tool_execution_mode == ToolExecutionMode::Sequential
    {
        execute_prepared_tools_sequential(&context, preflights, &mut output).await?
    } else {
        execute_prepared_tools_parallel(&context, preflights, &mut output).await?
    };
    append_tool_execution_effects(&mut output, &continuation.run_id, source_order_outputs);
    Ok(output)
}

fn set_tool_approval_pause(
    output: &mut PhaseOutput,
    run_id: &str,
    turn_id: u64,
    tool_execution_mode: ToolExecutionMode,
    preflights: Vec<ToolApprovalPreflight>,
    approval_requests: Vec<ToolApprovalRequest>,
) {
    output.tool_approval_requests = approval_requests;
    output.pause = Some(RunPauseReason::ToolApproval {
        continuation: ToolApprovalContinuation {
            run_id: run_id.to_string(),
            turn_id,
            phase: PHASE_TOOL_EXECUTE.into(),
            scratch: output.scratch.clone(),
            tool_execution_mode,
            preflights,
        },
    });
}

fn append_tool_execution_effects(
    output: &mut PhaseOutput,
    run_id: &str,
    source_order_outputs: Vec<(ToolCall, ToolOutput)>,
) {
    for (tool_call, tool_output) in source_order_outputs {
        output
            .scratch
            .tool_outputs
            .push((tool_call.clone(), tool_output.clone()));
        output.effects.push(AgentEffect::AppendMessage {
            message: AgentMessage::tool_result(
                format!("tool-result-{}-{}", run_id, tool_call.id),
                tool_call.id,
                tool_call.name,
                tool_output,
            ),
        });
    }
}

fn tool_execution_mode_for_calls(
    context: &PhaseContext<'_>,
    tool_calls: &[ToolCall],
) -> ToolExecutionMode {
    if context.runtime.tool_execution_mode() == ToolExecutionMode::Sequential
        || tool_calls.iter().any(|tool_call| {
            context
                .runtime
                .tool(&tool_call.name)
                .ok()
                .and_then(|tool| tool.spec().execution_mode)
                == Some(ToolExecutionMode::Sequential)
        })
    {
        ToolExecutionMode::Sequential
    } else {
        ToolExecutionMode::Parallel
    }
}

async fn prepare_tool_preflights(
    handles: ToolRuntimeHandles,
    run_id: &str,
    turn_id: u64,
    state: &AgentState,
    tool_calls: &[ToolCall],
    tool_execution_mode: ToolExecutionMode,
    cancellation: CancellationToken,
) -> Result<(Vec<ToolApprovalPreflight>, Vec<ToolApprovalRequest>)> {
    let context = ToolPreflightContext {
        handles,
        run_id: run_id.to_string(),
        turn_id,
        state: state.clone(),
        cancellation,
    };
    if tool_execution_mode == ToolExecutionMode::Parallel {
        return prepare_tool_preflights_parallel(context, tool_calls).await;
    }
    prepare_tool_preflights_sequential(&context, tool_calls).await
}

async fn prepare_tool_preflights_sequential(
    context: &ToolPreflightContext,
    tool_calls: &[ToolCall],
) -> Result<(Vec<ToolApprovalPreflight>, Vec<ToolApprovalRequest>)> {
    let mut preflights = Vec::with_capacity(tool_calls.len());
    let mut approval_requests = Vec::new();
    for tool_call in tool_calls {
        let (preflight, approval_request) =
            prepare_one_tool_call(context, tool_call.clone(), 0, None).await?;
        if let Some(approval_request) = approval_request {
            approval_requests.push(approval_request);
        }
        preflights.push(preflight);
    }
    Ok((preflights, approval_requests))
}

async fn prepare_tool_preflights_parallel(
    context: ToolPreflightContext,
    tool_calls: &[ToolCall],
) -> Result<(Vec<ToolApprovalPreflight>, Vec<ToolApprovalRequest>)> {
    let mut tasks = JoinSet::new();
    let tool_call_count = tool_calls.len();
    for (index, tool_call) in tool_calls.iter().cloned().enumerate() {
        let context = context.clone();
        tasks.spawn(async move {
            let result = prepare_one_tool_call(&context, tool_call, 0, None).await;
            (index, result)
        });
    }

    let mut source_order_preflights = vec![None; tool_call_count];
    while let Some(joined) = tasks.join_next().await {
        let (index, result) = match joined {
            Ok(result) => result,
            Err(error) => {
                tasks.abort_all();
                return Err(crate::AgentCoreError::Phase(format!(
                    "parallel tool preflight task failed: {error}"
                )));
            }
        };
        match result {
            Ok(preflight) => {
                source_order_preflights[index] = Some(preflight);
            }
            Err(error) => {
                tasks.abort_all();
                return Err(error);
            }
        }
    }

    let mut preflights = Vec::with_capacity(tool_call_count);
    let mut approval_requests = Vec::new();
    for entry in source_order_preflights {
        let (preflight, approval_request) = entry.ok_or_else(|| {
            crate::AgentCoreError::Phase("parallel tool preflight result missing".into())
        })?;
        if let Some(approval_request) = approval_request {
            approval_requests.push(approval_request);
        }
        preflights.push(preflight);
    }
    Ok((preflights, approval_requests))
}

#[derive(Clone)]
struct ToolPreflightContext {
    handles: ToolRuntimeHandles,
    run_id: String,
    turn_id: u64,
    state: AgentState,
    cancellation: CancellationToken,
}

async fn prepare_one_tool_call(
    context: &ToolPreflightContext,
    tool_call: ToolCall,
    start_hook_index: usize,
    permission_audit: Option<ToolPermissionAudit>,
) -> Result<(ToolApprovalPreflight, Option<ToolApprovalRequest>)> {
    context.cancellation.throw_if_cancelled()?;
    let tool = context
        .handles
        .tools
        .get(&tool_call.name)
        .cloned()
        .ok_or_else(|| crate::AgentCoreError::MissingTool(tool_call.name.clone()))?;
    let tool_spec = tool.spec();
    let mut permission_audit = permission_audit.unwrap_or_else(|| ToolPermissionAudit {
        tool_call: tool_call.clone(),
        permissions: tool_spec.permissions.clone(),
        decisions: Vec::new(),
    });
    for (hook_index, hook) in context
        .handles
        .hooks
        .iter()
        .enumerate()
        .skip(start_hook_index)
    {
        let result = hook
            .before_tool_call(
                BeforeToolCallContext {
                    run_id: context.run_id.clone(),
                    turn_id: context.turn_id,
                    tool_call: tool_call.clone(),
                    tool_spec: tool_spec.clone(),
                    state: context.state.clone(),
                },
                context.cancellation.clone(),
            )
            .await?;
        let Some(result) = result else {
            continue;
        };
        match result {
            BeforeToolCallResult::Decision { decision } => {
                permission_audit
                    .decisions
                    .push(ToolPermissionDecisionRecord {
                        hook_id: hook.id().map(ToString::to_string),
                        decision: decision.clone(),
                    });
                if matches!(decision.outcome, ToolPermissionOutcome::Deny) {
                    return Ok((
                        ToolApprovalPreflight {
                            tool_call,
                            permission_audit,
                            status: ToolApprovalPreflightStatus::Denied { decision },
                        },
                        None,
                    ));
                }
            }
            BeforeToolCallResult::Approval { approval: request } => {
                let approval_id =
                    tool_approval_id(&context.run_id, context.turn_id, &tool_call.id, hook_index);
                let approval = ToolApprovalRequest {
                    approval_id: approval_id.clone(),
                    tool_call: tool_call.clone(),
                    permissions: tool_spec.permissions.clone(),
                    hook_id: hook.id().map(ToString::to_string),
                    request,
                };
                return Ok((
                    ToolApprovalPreflight {
                        tool_call,
                        permission_audit,
                        status: ToolApprovalPreflightStatus::Pending {
                            approval_id,
                            hook_index,
                            hook_id: hook.id().map(ToString::to_string),
                        },
                    },
                    Some(approval),
                ));
            }
        }
    }
    Ok((
        ToolApprovalPreflight {
            tool_call,
            permission_audit,
            status: ToolApprovalPreflightStatus::Ready,
        },
        None,
    ))
}

fn tool_approval_id(run_id: &str, turn_id: u64, tool_call_id: &str, hook_index: usize) -> String {
    format!("approval-{run_id}-{turn_id}-{tool_call_id}-{hook_index}")
}

async fn execute_prepared_tools_sequential(
    context: &PhaseContext<'_>,
    preflights: Vec<ToolApprovalPreflight>,
    output: &mut PhaseOutput,
) -> Result<Vec<(ToolCall, ToolOutput)>> {
    let mut source_order_outputs = Vec::new();
    let handles = context.runtime.tool_handles();
    for preflight in preflights {
        let tool_call = preflight.tool_call.clone();
        let execution = execute_prepared_tool_call(
            handles.clone(),
            context.run_id.to_string(),
            context.turn_id,
            context.state.clone(),
            preflight,
            context.cancellation.clone(),
        )
        .await?;
        output
            .completed_tool_outputs
            .push((tool_call.clone(), execution.output.clone()));
        output
            .completed_tool_permission_audits
            .push(execution.permission_audit.clone());
        source_order_outputs.push((tool_call, execution.output));
    }
    Ok(source_order_outputs)
}

async fn execute_prepared_tools_parallel(
    context: &PhaseContext<'_>,
    preflights: Vec<ToolApprovalPreflight>,
    output: &mut PhaseOutput,
) -> Result<Vec<(ToolCall, ToolOutput)>> {
    let mut tasks = JoinSet::new();
    let handles = context.runtime.tool_handles();
    let preflight_count = preflights.len();
    for (index, preflight) in preflights.into_iter().enumerate() {
        let run_id = context.run_id.to_string();
        let handles = handles.clone();
        let state = context.state.clone();
        let cancellation = context.cancellation.clone();
        let turn_id = context.turn_id;
        let tool_call = preflight.tool_call.clone();
        tasks.spawn(async move {
            let result = execute_prepared_tool_call(
                handles,
                run_id,
                turn_id,
                state,
                preflight,
                cancellation,
            )
            .await;
            (index, tool_call, result)
        });
    }

    let mut source_order_outputs = vec![None; preflight_count];
    while let Some(joined) = tasks.join_next().await {
        let (index, tool_call, result) = match joined {
            Ok(result) => result,
            Err(error) => {
                tasks.abort_all();
                return Err(crate::AgentCoreError::Phase(format!(
                    "parallel tool execution task failed: {error}"
                )));
            }
        };
        let execution = match result {
            Ok(execution) => execution,
            Err(error) => {
                tasks.abort_all();
                return Err(error);
            }
        };
        output
            .completed_tool_outputs
            .push((tool_call.clone(), execution.output.clone()));
        output
            .completed_tool_permission_audits
            .push(execution.permission_audit.clone());
        source_order_outputs[index] = Some((tool_call, execution.output));
    }

    source_order_outputs
        .into_iter()
        .map(|entry| {
            entry.ok_or_else(|| crate::AgentCoreError::Phase("parallel tool result missing".into()))
        })
        .collect()
}

async fn execute_prepared_tool_call(
    handles: ToolRuntimeHandles,
    run_id: String,
    turn_id: u64,
    state: AgentState,
    preflight: ToolApprovalPreflight,
    cancellation: CancellationToken,
) -> Result<ToolExecutionOutcome> {
    cancellation.throw_if_cancelled()?;
    let tool_call = preflight.tool_call.clone();
    let permission_audit = preflight.permission_audit;
    if let ToolApprovalPreflightStatus::Denied { decision } = preflight.status {
        return Ok(ToolExecutionOutcome {
            output: denied_tool_output(&decision),
            permission_audit,
        });
    }
    if matches!(
        preflight.status,
        ToolApprovalPreflightStatus::Pending { .. }
    ) {
        return Err(crate::AgentCoreError::Phase(
            "cannot execute tool with pending approval".into(),
        ));
    }
    let tool = handles
        .tools
        .get(&tool_call.name)
        .cloned()
        .ok_or_else(|| crate::AgentCoreError::MissingTool(tool_call.name.clone()))?;

    let request = crate::ToolRequest {
        run_id: run_id.clone(),
        turn_id,
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        arguments: tool_call.arguments.clone(),
        state: state.clone(),
    };
    let mut output = match tool.execute_tool(request, cancellation.clone()).await {
        Ok(output) => output,
        Err(crate::AgentCoreError::Aborted) => return Err(crate::AgentCoreError::Aborted),
        Err(error) => error_tool_output(error.to_string()),
    };

    for hook in &handles.hooks {
        if let Some(rewrite) = hook
            .after_tool_call(
                AfterToolCallContext {
                    run_id: run_id.clone(),
                    turn_id,
                    tool_call: tool_call.clone(),
                    output: output.clone(),
                    state: state.clone(),
                },
                cancellation.clone(),
            )
            .await?
        {
            if let Some(content) = rewrite.content {
                output.content = content;
            }
            if let Some(details) = rewrite.details {
                output.details = details;
            }
            if let Some(is_error) = rewrite.is_error {
                output.is_error = is_error;
            }
        }
    }

    Ok(ToolExecutionOutcome {
        output,
        permission_audit,
    })
}

struct ToolExecutionOutcome {
    output: ToolOutput,
    permission_audit: ToolPermissionAudit,
}

fn denied_tool_output(decision: &ToolPermissionDecision) -> ToolOutput {
    let mut output = error_tool_output(
        decision
            .reason
            .clone()
            .unwrap_or_else(|| "tool execution was denied".into()),
    );
    output.details = json!({ "permissionDecision": decision });
    output
}

fn error_tool_output(message: String) -> ToolOutput {
    ToolOutput {
        content: vec![ContentBlock::Text { text: message }],
        details: json!({}),
        is_error: true,
        updates: Vec::new(),
    }
}

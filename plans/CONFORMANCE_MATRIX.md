# Agent Core Conformance Matrix

本文件是 `noloong-agent-core` 的能力到验证证据映射。新增核心行为时，必须先在这里登记能力、不变量、测试名和 gate；如果能力暂时没有覆盖，必须标记为 explicit gap。

## Default Local Gate

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p noloong-agent-core --examples
node --check crates/noloong-agent-core/tests/fixtures/stdio-extension.mjs
node --check crates/noloong-agent-core/tests/fixtures/openrouter-deepseek-extension.mjs
node --check examples/extensions/ai-sdk-provider/stdio-ai-sdk-extension.mjs
```

## Manual External Gate

```bash
cargo test -p noloong-agent-core --test openrouter_live -- --ignored --nocapture
```

该 gate 需要 `OPENROUTER_API_KEY`，固定使用 `deepseek/deepseek-v4-flash`，并通过 OpenRouter provider-only 路由要求 DeepSeek official provider 和 thinking enabled。OpenRouter/DeepSeek 参数只在测试侧通过 generic `ChatCompletionsProviderConfig` 组装，不能进入 core provider 硬编码 preset。测试使用足够的 live output budget 观察 thinking、visible text 和 tool-call streaming 的组合行为。默认 CI 不运行该 gate。

## Capability Matrix

| Capability | Required invariant | Test evidence | Gate | Gap |
|---|---|---|---|---|
| Runtime event sourcing | Successful report state equals reducer replay | `event_log_replays_to_report_state`, `runtime_success_replay_matches_report_state` | `cargo test --workspace` | None |
| Runtime failure replay | Phase/model/context failures record failed replay state | `model_stream_failure_records_failed_replay_state`, `context_failure_records_failed_replay_state`, `phase_failure_records_failed_replay_state`, `runtime_failure_records_failed_replay_state` | `cargo test --workspace` | None |
| Runtime abort replay | Abort emits `RunAborted` and replays to `RunStatus::Aborted` | `agent_abort_cancels_active_run`, `runtime_abort_records_aborted_replay_state` | `cargo test --workspace` | None |
| Realtime event sink | Store append happens before sink notification | `run_with_events_emits_realtime_events_in_order`, `event_store_contains_event_before_sink_notification` | `cargo test --workspace` | None |
| Event sink failure | Failing sink terminates run and records `RunFailed` without notifying failed sink again | `event_sink_failure_records_run_failed`, `event_sink_failure_does_not_notify_run_failed_to_failing_sink` | `cargo test --workspace` | None |
| Stateful agent UX | `Agent` persists state across prompts and validates continuation | `agent_prompt_preserves_transcript_across_runs`, `agent_continue_run_validates_last_message_role` | `cargo test --workspace` | None |
| Agent run settlement | `wait_for_idle` waits for subscriber completion | `wait_for_idle_waits_for_subscriber_barrier` | `cargo test --workspace` | None |
| Follow-up queues | `OneAtATime` drains one message per turn; `All` drains all pending messages | `follow_up_runs_after_agent_would_stop`, `queue_mode_all_drains_multiple_follow_ups_into_one_turn`, `queue_one_at_a_time_drains_multiple_follow_ups_across_turns` | `cargo test --workspace` | None |
| Steering queues | Steering is injected after the current tool batch and seen by next turn | `steering_is_injected_after_tool_batch`, `steering_waits_until_tool_batch_completes` | `cargo test --workspace` | None |
| Tool execution policy | Parallel emits completion order but commits source order; sequential variants commit source order | `parallel_tools_emit_completion_order_but_commit_source_order`, `sequential_tools_emit_source_order`, `per_tool_execution_mode_can_force_sequential`, `tool_policy_modes_commit_source_order` | `cargo test --workspace` | None |
| Tool hooks | `before_tool_call` can block; `after_tool_call` can rewrite output | `tool_hooks_can_block_and_rewrite_results` | `cargo test --workspace` | None |
| JSON-RPC lifecycle | initialize/capabilities/shutdown work over stdio | `stdio_extension_supports_lifecycle_methods` | `cargo test --workspace` | None |
| JSON-RPC provider/tool/context/phase | stdio extension can contribute all extension kinds | `stdio_extension_runs_provider_tool_and_context` | `cargo test --workspace` | None |
| JSON-RPC incremental streaming | stream notification reaches runtime before request response | `stdio_model_stream_notifications_are_incremental`, `jsonrpc_stream_event_arrives_before_response` | `cargo test --workspace` | None |
| JSON-RPC terminal lifecycle | `Finished` can settle stream before JSON-RPC response | `stdio_model_stream_can_finish_before_jsonrpc_response`, `jsonrpc_finished_settles_without_response` | `cargo test --workspace` | None |
| JSON-RPC error lifecycle | `Failed`, invalid JSON, extension crash, request timeout, and stream timeout are structured failures | `stdio_model_stream_error_records_failed_replay_state`, `invalid_json_from_stdio_extension_is_reported`, `stdio_extension_crash_records_failed_replay_state`, `jsonrpc_request_timeout_is_structured`, `stdio_model_stream_timeout_is_separate_from_request_timeout` | `cargo test --workspace` | None |
| Structured thinking | Thinking events/content preserve display text, raw JSON/object/list, summary kind, and replay descriptor | `thinking_type_serde_round_trips_structured_payloads`, `thinking_details_preserve_raw_json_and_render_summary_delta`, `object_reasoning_preserves_raw_snapshot_and_summary_kind`, `arbitrary_object_reasoning_preserves_raw_snapshot_without_text` | `cargo test --workspace` | None |
| Built-in Chat Completions payload | Messages, tool specs, tool results, provider extra body, and scoped thinking replay map to compatible Chat Completions JSON | `payload_maps_messages_tools_and_replay_descriptor`, `payload_does_not_replay_reasoning_across_provider_scope`, `config_carries_provider_specific_body_without_core_presets` | `cargo test --workspace` | None |
| Built-in Chat Completions streaming | SSE text, thinking, tool calls, legacy function calls, finish reasons, HTTP errors, request timeout, and cancellation map to core events/errors | `sse_streams_text_thinking_tool_calls_and_finish_reason`, `legacy_function_call_streams_tool_use`, `content_filter_maps_to_error_finish_reason`, `http_error_reports_status_and_body_excerpt`, `request_timeout_applies_before_initial_response`, `cancellation_aborts_pending_request` | `cargo test --workspace` | None |
| Built-in provider vendor neutrality | Core provider source contains no OpenRouter/DeepSeek-specific constants or constructors | `rg -n "openrouter|deepseek|OPENROUTER|deepseek-v4" crates/noloong-agent-core/src -S` returns no matches | manual audit | None |
| OpenRouter live thinking/tools | Real model path returns thinking and commits thinking content; built-in provider route can also stream visible text and a tool call through the same generic Chat Completions adapter | `openrouter_deepseek_v4_flash_official_provider_with_builtin_chat_completions`, `openrouter_deepseek_v4_flash_official_provider_with_thinking` | manual external gate | None |

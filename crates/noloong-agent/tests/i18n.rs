use noloong_agent::{
    BuiltInToolName, Catalog, HostEnvironment, HostProcessCompletion, HostProcessManager,
    JobSnapshot, JobStatus, Locale, MessageKey, ProcessOutput, i18n::ToolOutputOverflowRender,
};
use noloong_agent_core::ToolCall;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn i18n_catalog_has_all_english_keys() {
    Catalog::assert_complete(Locale::En);
}

#[test]
fn i18n_catalog_has_all_chinese_keys() {
    Catalog::assert_complete(Locale::Zh);
}

#[test]
fn renders_host_environment_context() {
    let environment = HostEnvironment::detect(Some(Locale::En));
    let rendered = Catalog::new(Locale::En).render_host_environment(&environment);

    assert!(
        rendered.contains(Catalog::new(Locale::En).message(MessageKey::HostEnvironmentContext))
    );
    assert!(rendered.contains(&environment.os));
    assert!(rendered.contains(&environment.default_shell));
}

#[test]
fn renders_agent_visible_dynamic_messages_in_chinese() {
    let catalog = Catalog::new(Locale::Zh);
    let completion = HostProcessCompletion {
        snapshot: JobSnapshot {
            job_id: "job-1".into(),
            command: "printf ok".into(),
            shell: "sh".into(),
            cwd: PathBuf::from("."),
            status: JobStatus::Exited { code: Some(0) },
            started_at_ms: 1,
            ended_at_ms: Some(2),
            next_cursor: 1,
            dropped_before_seq: 0,
        },
        output: ProcessOutput {
            job_id: "job-1".into(),
            chunks: Vec::new(),
            next_cursor: 1,
            dropped_before_seq: 0,
            truncated: false,
            status: JobStatus::Exited { code: Some(0) },
        },
    };
    let completion_text = catalog.render_background_completion(&completion, "ok");
    let overflow_path = PathBuf::from("/tmp/output.json");
    let overflow_text = catalog.render_tool_output_overflow(ToolOutputOverflowRender {
        path: overflow_path.as_path(),
        tool_name: "large.output",
        tool_call_id: "call-1",
        original_bytes: 1024,
        inline_limit_bytes: 128,
        preview_head: "head",
        preview_tail: "tail",
        preview_omitted_bytes: 768,
    });
    let approval_prompt = catalog.render_approval_prompt(&ToolCall {
        id: "call-1".into(),
        name: BuiltInToolName::HostExecStart.as_str().into(),
        arguments: json!({"command": "pwd"}),
    });

    assert!(completion_text.contains("后台宿主机命令已完成"));
    assert!(!completion_text.contains("Background host command completed"));
    assert!(overflow_text.contains("工具输出过长"));
    assert!(!overflow_text.contains("Tool output was too large"));
    assert!(approval_prompt.contains("工具："));
    assert!(!approval_prompt.contains("Tool:"));
}

#[tokio::test]
async fn host_tool_errors_use_catalog_locale() {
    let manager = HostProcessManager::new();
    let start = noloong_agent::HostExecStartTool::new(manager, Catalog::new(Locale::Zh));
    let error = noloong_agent_core::ToolProvider::execute_tool(
        &start,
        noloong_agent_core::ToolRequest {
            run_id: "run-1".into(),
            turn_id: 1,
            tool_call_id: "call-1".into(),
            tool_name: BuiltInToolName::HostExecStart.as_str().into(),
            arguments: json!({"command": ""}),
            state: noloong_agent_core::AgentState::default(),
        },
        noloong_agent_core::CancellationToken::new(),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("命令不能为空"));
    assert!(!error.to_string().contains("command must not be empty"));
}

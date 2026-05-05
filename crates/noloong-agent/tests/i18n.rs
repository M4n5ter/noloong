use noloong_agent::{
    ApplyPatchTool, BuiltInToolName, Catalog, FileEditManager, HostEnvironment, HostExecListTool,
    HostExecReadTool, HostExecStartTool, HostExecTerminateTool, HostExecWaitTool,
    HostExecWriteTool, HostProcessCompletion, HostProcessManager, JobSnapshot, JobStatus, Locale,
    ManifestPatchProposalTool, ManifestProposalStore, MessageKey, ProcessOutput, WriteFileTool,
    i18n::ToolOutputOverflowRender,
};
use noloong_agent_core::{
    AgentCoreError, AgentState, CancellationToken, ToolCall, ToolProvider, ToolRequest, ToolSpec,
};
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
fn i18n_tool_specs_use_english_catalog_descriptions() {
    assert_tool_specs_use_catalog_descriptions(Locale::En);
}

#[test]
fn i18n_tool_specs_use_chinese_catalog_descriptions() {
    assert_tool_specs_use_catalog_descriptions(Locale::Zh);
}

#[test]
fn i18n_host_environment_renderer_uses_catalog_header() {
    let environment = HostEnvironment::detect(Some(Locale::En));
    let catalog = Catalog::new(Locale::En);
    let rendered = catalog.render_host_environment(&environment);

    assert_eq!(
        rendered.lines().next(),
        Some(catalog.message(MessageKey::HostEnvironmentContext))
    );
    assert_eq!(rendered.lines().count(), 8);
}

#[test]
fn i18n_dynamic_renderers_respect_locale_selection() {
    let english = Catalog::new(Locale::En);
    let chinese = Catalog::new(Locale::Zh);
    let completion = test_completion();
    let approval = ToolCall {
        id: "call-1".into(),
        name: BuiltInToolName::HostExecStart.as_str().into(),
        arguments: json!({"command": "pwd"}),
    };

    assert_ne!(
        chinese.render_background_completion(&completion, "ok"),
        english.render_background_completion(&completion, "ok")
    );
    let overflow_path = PathBuf::from("/tmp/output.json");
    assert_ne!(
        chinese.render_tool_output_overflow(test_overflow(overflow_path.as_path())),
        english.render_tool_output_overflow(test_overflow(overflow_path.as_path()))
    );
    assert_ne!(
        chinese.render_approval_prompt(&approval),
        english.render_approval_prompt(&approval)
    );
}

#[tokio::test]
async fn i18n_host_tool_errors_use_catalog_locale() {
    let manager = HostProcessManager::new();
    let start = HostExecStartTool::new(manager, Catalog::new(Locale::Zh));
    let error = ToolProvider::execute_tool(
        &start,
        ToolRequest {
            run_id: "run-1".into(),
            turn_id: 1,
            tool_call_id: "call-1".into(),
            tool_name: BuiltInToolName::HostExecStart.as_str().into(),
            arguments: json!({"command": ""}),
            state: AgentState::default(),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap_err();

    let AgentCoreError::Provider(message) = error else {
        panic!("expected provider error");
    };
    assert_eq!(
        message,
        Catalog::new(Locale::Zh).command_must_not_be_empty()
    );
    assert_ne!(
        message,
        Catalog::new(Locale::En).command_must_not_be_empty()
    );
}

fn assert_tool_specs_use_catalog_descriptions(locale: Locale) {
    let catalog = Catalog::new(locale);
    let process_manager = HostProcessManager::new();

    assert_tool_spec_text(
        &HostExecStartTool::new(process_manager.clone(), catalog.clone()).spec(),
        catalog.message(MessageKey::HostExecStartDescription),
        catalog.message(MessageKey::HostCommandPermissionDescription),
    );
    assert_tool_spec_text(
        &HostExecReadTool::new(process_manager.clone(), catalog.clone()).spec(),
        catalog.message(MessageKey::HostExecReadDescription),
        catalog.message(MessageKey::HostCommandPermissionDescription),
    );
    assert_tool_spec_text(
        &HostExecWaitTool::new(process_manager.clone(), catalog.clone()).spec(),
        catalog.message(MessageKey::HostExecWaitDescription),
        catalog.message(MessageKey::HostCommandPermissionDescription),
    );
    assert_tool_spec_text(
        &HostExecWriteTool::new(process_manager.clone(), catalog.clone()).spec(),
        catalog.message(MessageKey::HostExecWriteDescription),
        catalog.message(MessageKey::HostCommandPermissionDescription),
    );
    assert_tool_spec_text(
        &HostExecTerminateTool::new(process_manager.clone(), catalog.clone()).spec(),
        catalog.message(MessageKey::HostExecTerminateDescription),
        catalog.message(MessageKey::HostCommandPermissionDescription),
    );
    assert_tool_spec_text(
        &HostExecListTool::new(process_manager, catalog.clone()).spec(),
        catalog.message(MessageKey::HostExecListDescription),
        catalog.message(MessageKey::HostCommandPermissionDescription),
    );

    let file_manager = FileEditManager::new(".");
    assert_tool_spec_text(
        &WriteFileTool::new(file_manager.clone(), catalog.clone()).spec(),
        catalog.message(MessageKey::FileWriteDescription),
        catalog.message(MessageKey::FileEditPermissionDescription),
    );
    assert_tool_spec_text(
        &ApplyPatchTool::new(file_manager, catalog.clone()).spec(),
        catalog.message(MessageKey::FileApplyPatchDescription),
        catalog.message(MessageKey::FileEditPermissionDescription),
    );

    assert_tool_spec_text(
        &ManifestPatchProposalTool::new(ManifestProposalStore::default(), catalog.clone()).spec(),
        catalog.message(MessageKey::ManifestPatchDescription),
        catalog.message(MessageKey::ManifestPatchPermissionDescription),
    );
}

fn assert_tool_spec_text(spec: &ToolSpec, description: &str, permission_description: &str) {
    assert_eq!(spec.description, description);
    assert_eq!(spec.permissions.len(), 1);
    assert_eq!(
        spec.permissions[0].description.as_deref(),
        Some(permission_description)
    );
}

fn test_completion() -> HostProcessCompletion {
    HostProcessCompletion {
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
    }
}

fn test_overflow(path: &std::path::Path) -> ToolOutputOverflowRender<'_> {
    ToolOutputOverflowRender {
        path,
        tool_name: "large.output",
        tool_call_id: "call-1",
        original_bytes: 1024,
        inline_limit_bytes: 128,
        preview_head: "head",
        preview_tail: "tail",
        preview_omitted_bytes: 768,
    }
}

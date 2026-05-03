use noloong_agent_core::{
    CONFORMANCE_COMPACTION_SUMMARIZER_ID, CONFORMANCE_CONTEXT_PROVIDER_ID,
    CONFORMANCE_MODEL_PROVIDER_ID, CONFORMANCE_PHASE_HOOK_ID, CONFORMANCE_PHASE_NODE_ID,
    CONFORMANCE_TOOL_CALL_HOOK_ID, CONFORMANCE_TOOL_NAME,
};
pub mod support;
use std::{fs, path::PathBuf};
use support::workspace_root;

#[test]
fn extension_docs_cover_current_contract() {
    let root = workspace_root();
    let extension_docs = read_to_string(root.join("crates/noloong-agent-core/docs/EXTENSIONS.md"));
    let typescript_readme =
        read_to_string(root.join("examples/extensions/typescript-conformance/README.md"));
    let python_readme =
        read_to_string(root.join("examples/extensions/python-conformance/README.md"));
    let example_docs = format!("{typescript_readme}\n{python_readme}");

    assert_contains_all(
        "extension method",
        &extension_docs,
        &[
            "initialize",
            "capabilities/list",
            "model/stream",
            "stream/event",
            "tool/execute",
            "context/apply",
            "phase/run",
            "phase_hook/run",
            "tool_hook/run",
            "compaction/summarize",
            "shutdown",
        ],
    );

    assert_contains_all(
        "hook point",
        &extension_docs,
        &[
            "before_model_request",
            "after_model_request",
            "before_assistant_commit",
            "after_assistant_commit",
            "before_tool_call",
            "after_tool_call",
        ],
    );

    assert_contains_all(
        "shared shape",
        &extension_docs,
        &[
            "AgentState",
            "AgentMessage",
            "ContentBlock",
            "ThinkingBlock",
            "MediaBlock",
            "ModelStreamEvent",
            "ToolSpec",
            "ToolCall",
            "ToolOutput",
            "AgentEffect",
            "PhaseScratch",
            "PhaseOutput",
            "ToolPermissionDecision",
            "CompactionSummaryRequest",
            "CompactionSummaryResult",
        ],
    );

    assert_contains_all(
        "strict conformance id",
        &extension_docs,
        &[
            CONFORMANCE_MODEL_PROVIDER_ID,
            CONFORMANCE_TOOL_NAME,
            CONFORMANCE_CONTEXT_PROVIDER_ID,
            CONFORMANCE_PHASE_NODE_ID,
            CONFORMANCE_PHASE_HOOK_ID,
            CONFORMANCE_TOOL_CALL_HOOK_ID,
            CONFORMANCE_COMPACTION_SUMMARIZER_ID,
        ],
    );

    assert_contains_all(
        "example handler mapping",
        &example_docs,
        &[
            "EXTENSIONS.md",
            "model/stream",
            "phase_hook/run",
            "tool_hook/run",
            "compaction/summarize",
            "stream/event",
        ],
    );
}

fn assert_contains_all(kind: &str, text: &str, expected: &[&str]) {
    for token in expected {
        assert!(
            text.contains(token),
            "{kind} documentation is missing `{token}`"
        );
    }
}

fn read_to_string(path: PathBuf) -> String {
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

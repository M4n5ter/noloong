use noloong_agent::{
    Catalog, HostExecReadTool, HostExecStartTool, HostExecWaitTool, HostProcessManager, Locale,
};
use noloong_agent_core::{AgentState, CancellationToken, ContentBlock, ToolProvider, ToolRequest};
use serde_json::json;

#[tokio::test]
async fn host_exec_tools_start_fast_path_returns_result() {
    let manager = HostProcessManager::new();
    let start = HostExecStartTool::new(manager.clone(), Catalog::new(Locale::En));
    let output = start
        .execute_tool(
            request(json!({
                "command": "printf fast",
                "shell": "sh",
                "foregroundWaitMs": 1000
            })),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let details = output.details;
    let job_id = details["jobId"].as_str().unwrap();

    assert_eq!(details["status"]["state"], "exited");
    assert_eq!(details["nextCursor"], 1);

    let read = HostExecReadTool::new(manager, Catalog::new(Locale::En));
    let output = read
        .execute_tool(
            request(json!({"jobId": job_id, "afterSeq": 0})),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(joined_tool_text(&output), "fast");
}

#[tokio::test]
async fn host_exec_tools_start_and_read_background() {
    let manager = HostProcessManager::new();
    let start = HostExecStartTool::new(manager.clone(), Catalog::new(Locale::En));
    let output = start
        .execute_tool(
            request(json!({
                "command": "sleep 1; printf slow",
                "shell": "sh",
                "foregroundWaitMs": 10
            })),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let job_id = output.details["jobId"].as_str().unwrap().to_string();

    assert_eq!(output.details["status"]["state"], "running");

    let wait = HostExecWaitTool::new(manager.clone(), Catalog::new(Locale::En));
    let wait_output = wait
        .execute_tool(
            request(json!({"jobId": job_id, "timeoutMs": 3000})),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(wait_output.details["timedOut"], false);

    let read = HostExecReadTool::new(manager, Catalog::new(Locale::En));
    let output = read
        .execute_tool(
            request(json!({"jobId": job_id, "afterSeq": 0})),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(joined_tool_text(&output), "slow");
}

#[tokio::test]
async fn host_exec_tools_wait_timeout() {
    let manager = HostProcessManager::new();
    let start = HostExecStartTool::new(manager.clone(), Catalog::new(Locale::En));
    let output = start
        .execute_tool(
            request(json!({
                "command": "sleep 1",
                "shell": "sh",
                "foregroundWaitMs": 10
            })),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let job_id = output.details["jobId"].as_str().unwrap().to_string();

    let wait = HostExecWaitTool::new(manager, Catalog::new(Locale::En));
    let wait_output = wait
        .execute_tool(
            request(json!({"jobId": job_id, "timeoutMs": 10})),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(wait_output.details["timedOut"], true);
    assert_eq!(wait_output.details["status"]["state"], "running");
}

#[tokio::test]
async fn host_exec_output_cap_and_cursor() {
    let manager = HostProcessManager::new();
    let start = HostExecStartTool::new(manager.clone(), Catalog::new(Locale::En));
    let output = start
        .execute_tool(
            request(json!({
                "command": "printf first; sleep 0.05; printf second; sleep 0.05; printf third",
                "shell": "sh",
                "foregroundWaitMs": 1000
            })),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let job_id = output.details["jobId"].as_str().unwrap().to_string();

    let read = HostExecReadTool::new(manager, Catalog::new(Locale::En));
    let output = read
        .execute_tool(
            request(json!({"jobId": job_id, "afterSeq": 0, "maxBytes": 6})),
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(output.details["truncated"], true);
    assert!(output.details["nextCursor"].as_u64().unwrap() > 0);
    assert!(tool_output_byte_len(&output) <= 6);
}

fn request(arguments: serde_json::Value) -> ToolRequest {
    ToolRequest {
        run_id: "run-test".into(),
        turn_id: 1,
        tool_call_id: "tool-call-test".into(),
        tool_name: "host.exec.start".into(),
        arguments,
        state: AgentState::default(),
    }
}

fn joined_tool_text(output: &noloong_agent_core::ToolOutput) -> String {
    let ContentBlock::Json { value } = &output.content[0] else {
        panic!("expected json tool output");
    };
    value["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chunk| chunk["text"].as_str().unwrap())
        .collect()
}

fn tool_output_byte_len(output: &noloong_agent_core::ToolOutput) -> usize {
    let ContentBlock::Json { value } = &output.content[0] else {
        panic!("expected json tool output");
    };
    value["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chunk| chunk["byteLen"].as_u64().unwrap() as usize)
        .sum()
}

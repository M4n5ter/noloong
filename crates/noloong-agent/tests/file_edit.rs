use noloong_agent::{
    APPLY_PATCH_TOOL_NAME, ApplyPatchTool, Catalog, FILE_EDIT_PERMISSION_CAPABILITY,
    FileEditManager, Locale, WRITE_FILE_TOOL_NAME, WriteFileTool,
};
use noloong_agent_core::{
    AgentState, CancellationToken, ToolExecutionMode, ToolProvider, ToolRequest,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[tokio::test]
async fn write_file_creates_missing_parents_and_writes_content() {
    let temp = TestDir::new("write-new");
    let tool = write_file_tool(&temp);

    let output = execute(
        &tool,
        json!({
            "path": "nested/file.txt",
            "content": "hello"
        }),
    )
    .await;

    assert!(!output.is_error);
    assert_eq!(
        fs::read_to_string(temp.path().join("nested/file.txt")).unwrap(),
        "hello"
    );
    assert_eq!(output.details["mode"], "write");
    assert_eq!(output.details["bytesWritten"], 5);
    assert_eq!(output.details["createdParentDirs"], true);
}

#[tokio::test]
async fn write_file_overwrites_existing_content() {
    let temp = TestDir::new("write-overwrite");
    fs::write(temp.path().join("file.txt"), "before").unwrap();
    let tool = write_file_tool(&temp);

    let output = execute(
        &tool,
        json!({
            "path": "file.txt",
            "content": "after"
        }),
    )
    .await;

    assert!(!output.is_error);
    assert_eq!(
        fs::read_to_string(temp.path().join("file.txt")).unwrap(),
        "after"
    );
}

#[tokio::test]
async fn write_file_replaces_exact_old_string() {
    let temp = TestDir::new("write-replace");
    fs::write(temp.path().join("file.txt"), "alpha\nbeta\ngamma\n").unwrap();
    let tool = write_file_tool(&temp);

    let output = execute(
        &tool,
        json!({
            "path": "file.txt",
            "oldString": "beta",
            "newString": "delta"
        }),
    )
    .await;

    assert!(!output.is_error);
    assert_eq!(
        fs::read_to_string(temp.path().join("file.txt")).unwrap(),
        "alpha\ndelta\ngamma\n"
    );
    assert_eq!(output.details["mode"], "replace");
    assert_eq!(output.details["replacements"], 1);
}

#[tokio::test]
async fn write_file_replace_all_is_explicit_for_multiple_matches() {
    let temp = TestDir::new("write-replace-all");
    let path = temp.path().join("file.txt");
    fs::write(&path, "same same").unwrap();
    let tool = write_file_tool(&temp);

    let ambiguous = execute(
        &tool,
        json!({
            "path": "file.txt",
            "oldString": "same",
            "newString": "diff"
        }),
    )
    .await;
    assert!(ambiguous.is_error);
    assert_eq!(fs::read_to_string(&path).unwrap(), "same same");

    let replaced = execute(
        &tool,
        json!({
            "path": "file.txt",
            "oldString": "same",
            "newString": "diff",
            "replaceAll": true
        }),
    )
    .await;
    assert!(!replaced.is_error);
    assert_eq!(fs::read_to_string(&path).unwrap(), "diff diff");
    assert_eq!(replaced.details["replacements"], 2);
}

#[tokio::test]
async fn write_file_rejects_directories_and_sensitive_paths() {
    let temp = TestDir::new("write-reject");
    fs::create_dir_all(temp.path().join("dir")).unwrap();
    let tool = write_file_tool(&temp);

    let directory = execute(
        &tool,
        json!({
            "path": "dir",
            "content": "nope"
        }),
    )
    .await;
    assert!(directory.is_error);

    let sensitive = execute(
        &tool,
        json!({
            "path": "/etc/noloong-agent-test",
            "content": "nope"
        }),
    )
    .await;
    assert!(sensitive.is_error);
}

#[tokio::test]
async fn apply_patch_adds_updates_deletes_and_moves_files() {
    let temp = TestDir::new("patch-basic");
    fs::write(temp.path().join("update.txt"), "hello\nold\nbye\n").unwrap();
    fs::write(temp.path().join("delete.txt"), "remove me\n").unwrap();
    fs::write(temp.path().join("move.txt"), "move me\n").unwrap();
    let tool = apply_patch_tool(&temp);

    let output = execute(
        &tool,
        json!({
            "patch": "*** Begin Patch\n*** Add File: added.txt\n+new file\n*** Update File: update.txt\n@@\n hello\n-old\n+new\n bye\n*** Delete File: delete.txt\n*** Move File: move.txt -> moved.txt\n*** End Patch"
        }),
    )
    .await;

    assert!(!output.is_error);
    assert_eq!(
        fs::read_to_string(temp.path().join("added.txt")).unwrap(),
        "new file\n"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("update.txt")).unwrap(),
        "hello\nnew\nbye\n"
    );
    assert!(!temp.path().join("delete.txt").exists());
    assert!(!temp.path().join("move.txt").exists());
    assert_eq!(
        fs::read_to_string(temp.path().join("moved.txt")).unwrap(),
        "move me\n"
    );
}

#[tokio::test]
async fn apply_patch_validation_failure_leaves_files_unchanged() {
    let temp = TestDir::new("patch-validation");
    let existing = temp.path().join("existing.txt");
    fs::write(&existing, "original\n").unwrap();
    let tool = apply_patch_tool(&temp);

    let output = execute(
        &tool,
        json!({
            "patch": "*** Begin Patch\n*** Add File: created.txt\n+created\n*** Update File: existing.txt\n@@\n-missing\n+changed\n*** End Patch"
        }),
    )
    .await;

    assert!(output.is_error);
    assert_eq!(fs::read_to_string(existing).unwrap(), "original\n");
    assert!(!temp.path().join("created.txt").exists());
}

#[tokio::test]
async fn apply_patch_rejects_malformed_patch() {
    let temp = TestDir::new("patch-malformed");
    let tool = apply_patch_tool(&temp);

    let output = execute(
        &tool,
        json!({ "patch": "*** Add File: a.txt\n+missing markers" }),
    )
    .await;

    assert!(output.is_error);
}

#[test]
fn file_edit_specs_are_sequential_and_permissioned() {
    let temp = TestDir::new("spec");
    let write_spec = write_file_tool(&temp).spec();
    let patch_spec = apply_patch_tool(&temp).spec();

    assert_eq!(write_spec.name, WRITE_FILE_TOOL_NAME);
    assert_eq!(patch_spec.name, APPLY_PATCH_TOOL_NAME);
    for spec in [write_spec, patch_spec] {
        assert_eq!(
            spec.permissions[0].capability,
            FILE_EDIT_PERMISSION_CAPABILITY
        );
        assert_eq!(spec.permissions[0].metadata["builtIn"], true);
        assert_eq!(spec.execution_mode, Some(ToolExecutionMode::Sequential));
    }
}

fn write_file_tool(temp: &TestDir) -> WriteFileTool {
    WriteFileTool::new(
        FileEditManager::new(temp.path().to_path_buf()),
        Catalog::new(Locale::En),
    )
}

fn apply_patch_tool(temp: &TestDir) -> ApplyPatchTool {
    ApplyPatchTool::new(
        FileEditManager::new(temp.path().to_path_buf()),
        Catalog::new(Locale::En),
    )
}

async fn execute(tool: &impl ToolProvider, arguments: Value) -> noloong_agent_core::ToolOutput {
    tool.execute_tool(
        ToolRequest {
            run_id: "run-test".into(),
            turn_id: 1,
            tool_call_id: "tool-call-test".into(),
            tool_name: tool.spec().name,
            arguments,
            state: AgentState::default(),
        },
        CancellationToken::new(),
    )
    .await
    .unwrap()
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "noloong-agent-file-edit-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

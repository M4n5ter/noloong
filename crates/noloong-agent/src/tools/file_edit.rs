use crate::{Catalog, MessageKey};
use noloong_agent_core::{
    AgentCoreError, BoxFuture, CancellationToken, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::Mutex as AsyncMutex;

use super::{json_tool_error, json_tool_output, sequential_tool_spec};

pub const WRITE_FILE_TOOL_NAME: &str = "write_file";
pub const APPLY_PATCH_TOOL_NAME: &str = "apply_patch";
pub const FILE_EDIT_PERMISSION_CAPABILITY: &str = "host.file.write";

#[derive(Clone)]
pub struct FileEditManager {
    root: Arc<PathBuf>,
    locks: Arc<Mutex<BTreeMap<PathBuf, Weak<AsyncMutex<()>>>>>,
}

#[derive(Clone)]
pub struct WriteFileTool {
    manager: FileEditManager,
    catalog: Catalog,
}

#[derive(Clone)]
pub struct ApplyPatchTool {
    manager: FileEditManager,
    catalog: Catalog,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedPath {
    requested: String,
    resolved: PathBuf,
}

#[derive(Debug)]
struct FileEditError {
    code: &'static str,
    message: String,
    details: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteFileInput {
    path: String,
    content: Option<String>,
    old_string: Option<String>,
    new_string: Option<String>,
    #[serde(default)]
    replace_all: bool,
}

enum WriteFileOperation {
    Write {
        content: String,
    },
    Replace {
        old_string: String,
        new_string: String,
        replace_all: bool,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyPatchInput {
    patch: String,
}

#[derive(Debug, PartialEq, Eq)]
struct PatchDocument {
    operations: Vec<PatchOperation>,
}

#[derive(Debug, PartialEq, Eq)]
enum PatchOperation {
    Add {
        path: String,
        content: String,
    },
    Update {
        path: String,
        hunks: Vec<PatchHunk>,
        move_to: Option<String>,
    },
    Delete {
        path: String,
    },
    Move {
        from: String,
        to: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct PatchHunk {
    old: String,
    new: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum StagedFile {
    Write(Vec<u8>),
    Delete,
}

impl FileEditManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(normalize_path(root.into())),
            locks: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    fn resolve(&self, path: &str) -> Result<ResolvedPath, FileEditError> {
        if path.trim().is_empty() {
            return Err(FileEditError::new(
                "invalid_path",
                "path must not be empty",
                json!({ "path": path }),
            ));
        }
        let raw_path = PathBuf::from(path);
        let resolved = if raw_path.is_absolute() {
            normalize_path(raw_path)
        } else {
            normalize_path(self.root.join(raw_path))
        };
        reject_sensitive_path(&resolved)?;
        reject_sensitive_path(&canonical_guard_path(&resolved))?;
        Ok(ResolvedPath {
            requested: path.into(),
            resolved,
        })
    }

    async fn write_text(&self, input: WriteFileInput) -> Result<Value, FileEditError> {
        let operation = input.operation()?;
        let path = self.resolve(&input.path)?;
        let _guards = self.lock_paths([path.resolved.clone()]).await;
        ensure_not_directory(&path.resolved).await?;
        match operation {
            WriteFileOperation::Write { content } => {
                let created_parent_dirs = create_parent_dirs(&path.resolved).await?;
                tokio::fs::write(&path.resolved, content.as_bytes())
                    .await
                    .map_err(|error| FileEditError::io("write_failed", &path.resolved, error))?;
                Ok(json!({
                    "mode": "write",
                    "path": path.requested,
                    "resolvedPath": path_string(&path.resolved),
                    "bytesWritten": content.len(),
                    "createdParentDirs": created_parent_dirs,
                }))
            }
            WriteFileOperation::Replace {
                old_string,
                new_string,
                replace_all,
            } => {
                let current = read_required_text(&path.resolved).await?;
                let (updated, replacements) = replace_string(
                    &current,
                    &old_string,
                    &new_string,
                    replace_all,
                    &path.resolved,
                )?;
                tokio::fs::write(&path.resolved, updated.as_bytes())
                    .await
                    .map_err(|error| FileEditError::io("write_failed", &path.resolved, error))?;
                Ok(json!({
                    "mode": "replace",
                    "path": path.requested,
                    "resolvedPath": path_string(&path.resolved),
                    "bytesWritten": updated.len(),
                    "createdParentDirs": false,
                    "replacements": replacements,
                }))
            }
        }
    }

    async fn apply_patch_text(&self, patch: &str) -> Result<Value, FileEditError> {
        let document = parse_patch(patch)?;
        let paths = self.resolve_patch_paths(&document)?;
        let _guards = self
            .lock_paths(paths.iter().map(|path| path.resolved.clone()))
            .await;
        let staged = self.validate_patch(&document, &paths).await?;
        self.commit_staged_files(&staged).await?;
        Ok(json!({
            "operations": patch_operation_summaries(&document),
            "paths": paths.iter().map(resolved_path_json).collect::<Vec<_>>(),
            "filesChanged": staged.len(),
        }))
    }

    fn resolve_patch_paths(
        &self,
        document: &PatchDocument,
    ) -> Result<Vec<ResolvedPath>, FileEditError> {
        let mut paths = BTreeMap::new();
        for operation in &document.operations {
            match operation {
                PatchOperation::Add { path, .. } | PatchOperation::Delete { path } => {
                    let resolved = self.resolve(path)?;
                    paths.insert(resolved.resolved.clone(), resolved);
                }
                PatchOperation::Update { path, move_to, .. } => {
                    let resolved = self.resolve(path)?;
                    paths.insert(resolved.resolved.clone(), resolved);
                    if let Some(move_to) = move_to {
                        let resolved = self.resolve(move_to)?;
                        paths.insert(resolved.resolved.clone(), resolved);
                    }
                }
                PatchOperation::Move { from, to } => {
                    let from = self.resolve(from)?;
                    let to = self.resolve(to)?;
                    paths.insert(from.resolved.clone(), from);
                    paths.insert(to.resolved.clone(), to);
                }
            }
        }
        Ok(paths.into_values().collect())
    }

    async fn validate_patch(
        &self,
        document: &PatchDocument,
        paths: &[ResolvedPath],
    ) -> Result<BTreeMap<PathBuf, StagedFile>, FileEditError> {
        let path_by_request = paths
            .iter()
            .map(|path| (path.requested.as_str(), path.resolved.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut staged = BTreeMap::new();
        for operation in &document.operations {
            match operation {
                PatchOperation::Add { path, content } => {
                    let resolved = path_by_request
                        .get(path.as_str())
                        .expect("patch path is resolved");
                    ensure_path_absent(resolved, &staged).await?;
                    staged.insert(resolved.clone(), StagedFile::Write(content.clone().into()));
                }
                PatchOperation::Update {
                    path,
                    hunks,
                    move_to,
                } => {
                    let resolved = path_by_request
                        .get(path.as_str())
                        .expect("patch path is resolved");
                    let mut content = read_staged_text(resolved, &staged).await?;
                    for hunk in hunks {
                        content = apply_hunk(&content, hunk, resolved)?;
                    }
                    if let Some(move_to) = move_to {
                        let destination = path_by_request
                            .get(move_to.as_str())
                            .expect("move destination is resolved");
                        ensure_path_absent(destination, &staged).await?;
                        staged.insert(resolved.clone(), StagedFile::Delete);
                        staged.insert(destination.clone(), StagedFile::Write(content.into()));
                    } else {
                        staged.insert(resolved.clone(), StagedFile::Write(content.into()));
                    }
                }
                PatchOperation::Delete { path } => {
                    let resolved = path_by_request
                        .get(path.as_str())
                        .expect("patch path is resolved");
                    ensure_path_exists(resolved, &staged).await?;
                    staged.insert(resolved.clone(), StagedFile::Delete);
                }
                PatchOperation::Move { from, to } => {
                    let source = path_by_request
                        .get(from.as_str())
                        .expect("move source is resolved");
                    let destination = path_by_request
                        .get(to.as_str())
                        .expect("move destination is resolved");
                    let content = read_staged_bytes(source, &staged).await?;
                    ensure_path_absent(destination, &staged).await?;
                    staged.insert(source.clone(), StagedFile::Delete);
                    staged.insert(destination.clone(), StagedFile::Write(content));
                }
            }
        }
        Ok(staged)
    }

    async fn commit_staged_files(
        &self,
        staged: &BTreeMap<PathBuf, StagedFile>,
    ) -> Result<(), FileEditError> {
        for (path, operation) in staged {
            match operation {
                StagedFile::Write(content) => {
                    create_parent_dirs(path).await?;
                    tokio::fs::write(path, content)
                        .await
                        .map_err(|error| FileEditError::io("write_failed", path, error))?;
                }
                StagedFile::Delete => {
                    tokio::fs::remove_file(path)
                        .await
                        .map_err(|error| FileEditError::io("delete_failed", path, error))?;
                }
            }
        }
        Ok(())
    }

    async fn lock_paths(
        &self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Vec<tokio::sync::OwnedMutexGuard<()>> {
        let locks = {
            let mut lock_map = self.locks.lock().expect("file edit lock map lock poisoned");
            lock_map.retain(|_, lock| lock.strong_count() > 0);
            sorted_unique_paths(paths)
                .into_iter()
                .map(|path| {
                    lock_map
                        .get(&path)
                        .and_then(Weak::upgrade)
                        .unwrap_or_else(|| {
                            let lock = Arc::new(AsyncMutex::new(()));
                            lock_map.insert(path, Arc::downgrade(&lock));
                            lock
                        })
                })
                .collect::<Vec<_>>()
        };
        let mut guards = Vec::with_capacity(locks.len());
        for lock in locks {
            guards.push(lock.lock_owned().await);
        }
        guards
    }
}

impl WriteFileTool {
    pub fn new(manager: FileEditManager, catalog: Catalog) -> Self {
        Self { manager, catalog }
    }
}

impl WriteFileInput {
    fn operation(&self) -> Result<WriteFileOperation, FileEditError> {
        match (&self.content, &self.old_string, &self.new_string) {
            (Some(content), None, None) => {
                if self.replace_all {
                    return Err(FileEditError::new(
                        "invalid_input",
                        "replaceAll is only valid with oldString and newString",
                        json!({ "path": self.path }),
                    ));
                }
                Ok(WriteFileOperation::Write {
                    content: content.clone(),
                })
            }
            (None, Some(old_string), Some(new_string)) => Ok(WriteFileOperation::Replace {
                old_string: old_string.clone(),
                new_string: new_string.clone(),
                replace_all: self.replace_all,
            }),
            (Some(_), Some(_), _) | (Some(_), _, Some(_)) => Err(FileEditError::new(
                "invalid_input",
                "write_file accepts either content or oldString/newString, not both",
                json!({ "path": self.path }),
            )),
            _ => Err(FileEditError::new(
                "invalid_input",
                "write_file requires either content or both oldString and newString",
                json!({ "path": self.path }),
            )),
        }
    }
}

impl ApplyPatchTool {
    pub fn new(manager: FileEditManager, catalog: Catalog) -> Self {
        Self { manager, catalog }
    }
}

impl ToolProvider for WriteFileTool {
    fn spec(&self) -> ToolSpec {
        file_edit_tool_spec(
            WRITE_FILE_TOOL_NAME,
            self.catalog.message(MessageKey::FileWriteDescription),
            json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "oldString": {"type": "string"},
                    "newString": {"type": "string"},
                    "replaceAll": {"type": "boolean"}
                },
                "oneOf": [
                    {
                        "required": ["content"],
                        "not": {
                            "anyOf": [
                                {"required": ["oldString"]},
                                {"required": ["newString"]},
                                {"required": ["replaceAll"]}
                            ]
                        }
                    },
                    {
                        "required": ["oldString", "newString"],
                        "not": {"required": ["content"]}
                    }
                ]
            }),
            &self.catalog,
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input = match serde_json::from_value::<WriteFileInput>(request.arguments) {
                Ok(input) => input,
                Err(error) => {
                    return Ok(file_edit_error_output(FileEditError::new(
                        "invalid_input",
                        format!("invalid write_file input: {error}"),
                        json!({}),
                    )));
                }
            };
            Ok(match self.manager.write_text(input).await {
                Ok(value) => json_tool_output(value),
                Err(error) => file_edit_error_output(error),
            })
        })
    }
}

impl ToolProvider for ApplyPatchTool {
    fn spec(&self) -> ToolSpec {
        file_edit_tool_spec(
            APPLY_PATCH_TOOL_NAME,
            self.catalog.message(MessageKey::FileApplyPatchDescription),
            json!({
                "type": "object",
                "required": ["patch"],
                "properties": {
                    "patch": {"type": "string"}
                }
            }),
            &self.catalog,
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let input = match serde_json::from_value::<ApplyPatchInput>(request.arguments) {
                Ok(input) => input,
                Err(error) => {
                    return Ok(file_edit_error_output(FileEditError::new(
                        "invalid_input",
                        format!("invalid apply_patch input: {error}"),
                        json!({}),
                    )));
                }
            };
            Ok(match self.manager.apply_patch_text(&input.patch).await {
                Ok(value) => json_tool_output(value),
                Err(error) => file_edit_error_output(error),
            })
        })
    }
}

impl FileEditError {
    fn new(code: &'static str, message: impl Into<String>, details: Value) -> Self {
        Self {
            code,
            message: message.into(),
            details,
        }
    }

    fn io(code: &'static str, path: &Path, error: std::io::Error) -> Self {
        Self::new(
            code,
            error.to_string(),
            json!({ "path": path_string(path) }),
        )
    }
}

impl Display for FileEditError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for FileEditError {}

fn file_edit_error_output(error: FileEditError) -> ToolOutput {
    json_tool_error(error.code, error.message, error.details)
}

fn file_edit_tool_spec(
    name: &str,
    description: &str,
    input_schema: Value,
    catalog: &Catalog,
) -> ToolSpec {
    sequential_tool_spec(
        name,
        description,
        input_schema,
        FILE_EDIT_PERMISSION_CAPABILITY,
        catalog.message(MessageKey::FileEditPermissionDescription),
    )
}

fn parse_patch(patch: &str) -> Result<PatchDocument, FileEditError> {
    let lines = patch.lines().collect::<Vec<_>>();
    if lines.first().copied() != Some("*** Begin Patch") {
        return Err(FileEditError::new(
            "malformed_patch",
            "patch must start with *** Begin Patch",
            json!({}),
        ));
    }
    if lines.last().copied() != Some("*** End Patch") {
        return Err(FileEditError::new(
            "malformed_patch",
            "patch must end with *** End Patch",
            json!({}),
        ));
    }
    let mut index = 1;
    let end = lines.len().saturating_sub(1);
    let mut operations = Vec::new();
    while index < end {
        let line = lines[index];
        if line.trim().is_empty() {
            index += 1;
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            let (content, next) = parse_add_file(&lines, index + 1, end)?;
            operations.push(PatchOperation::Add {
                path: path.to_owned(),
                content,
            });
            index = next;
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            let (hunks, move_to, next) = parse_update_file(&lines, index + 1, end)?;
            operations.push(PatchOperation::Update {
                path: path.to_owned(),
                hunks,
                move_to,
            });
            index = next;
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            operations.push(PatchOperation::Delete {
                path: path.to_owned(),
            });
            index += 1;
        } else if let Some(rest) = line.strip_prefix("*** Move File: ") {
            let (from, to) = rest.split_once(" -> ").ok_or_else(|| {
                FileEditError::new(
                    "malformed_patch",
                    "move operation must use *** Move File: source -> destination",
                    json!({ "line": line }),
                )
            })?;
            operations.push(PatchOperation::Move {
                from: from.to_owned(),
                to: to.to_owned(),
            });
            index += 1;
        } else {
            return Err(FileEditError::new(
                "malformed_patch",
                "unexpected patch line",
                json!({ "line": line }),
            ));
        }
    }
    if operations.is_empty() {
        return Err(FileEditError::new(
            "malformed_patch",
            "patch must contain at least one operation",
            json!({}),
        ));
    }
    Ok(PatchDocument { operations })
}

pub(crate) fn apply_patch_target_paths(patch: &str) -> Option<Vec<String>> {
    parse_patch(patch)
        .ok()
        .map(|document| document.target_paths())
}

impl PatchDocument {
    fn target_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        for operation in &self.operations {
            operation.collect_target_paths(&mut paths);
        }
        paths.sort();
        paths.dedup();
        paths
    }

    fn operation_summaries(&self) -> Vec<Value> {
        self.operations
            .iter()
            .map(PatchOperation::summary)
            .collect()
    }
}

impl PatchOperation {
    fn collect_target_paths(&self, paths: &mut Vec<String>) {
        match self {
            Self::Add { path, .. } | Self::Delete { path } => paths.push(path.clone()),
            Self::Update { path, move_to, .. } => {
                paths.push(path.clone());
                if let Some(move_to) = move_to {
                    paths.push(move_to.clone());
                }
            }
            Self::Move { from, to } => {
                paths.push(from.clone());
                paths.push(to.clone());
            }
        }
    }

    fn summary(&self) -> Value {
        match self {
            Self::Add { path, .. } => json!({ "op": "add", "path": path }),
            Self::Update { path, move_to, .. } => {
                json!({ "op": "update", "path": path, "moveTo": move_to })
            }
            Self::Delete { path } => json!({ "op": "delete", "path": path }),
            Self::Move { from, to } => json!({ "op": "move", "from": from, "to": to }),
        }
    }
}

fn parse_add_file(
    lines: &[&str],
    mut index: usize,
    end: usize,
) -> Result<(String, usize), FileEditError> {
    let mut content = String::new();
    while index < end && !is_operation_line(lines[index]) {
        let line = lines[index];
        if let Some(text) = line.strip_prefix('+') {
            push_patch_line(&mut content, text);
        } else if line == r"\ No newline at end of file" {
            trim_final_newline(&mut content);
        } else {
            return Err(FileEditError::new(
                "malformed_patch",
                "add file content lines must start with +",
                json!({ "line": line }),
            ));
        }
        index += 1;
    }
    Ok((content, index))
}

fn parse_update_file(
    lines: &[&str],
    mut index: usize,
    end: usize,
) -> Result<(Vec<PatchHunk>, Option<String>, usize), FileEditError> {
    let mut move_to = None;
    if index < end
        && let Some(path) = lines[index].strip_prefix("*** Move to: ")
    {
        move_to = Some(path.to_owned());
        index += 1;
    }
    let mut hunks = Vec::new();
    let mut old = String::new();
    let mut new = String::new();
    while index < end && !is_operation_line(lines[index]) {
        let line = lines[index];
        if line.starts_with("@@") {
            flush_hunk(&mut hunks, &mut old, &mut new);
        } else if let Some(text) = line.strip_prefix(' ') {
            push_patch_line(&mut old, text);
            push_patch_line(&mut new, text);
        } else if let Some(text) = line.strip_prefix('-') {
            push_patch_line(&mut old, text);
        } else if let Some(text) = line.strip_prefix('+') {
            push_patch_line(&mut new, text);
        } else if line == r"\ No newline at end of file" {
            trim_final_newline(&mut old);
            trim_final_newline(&mut new);
        } else if line.trim().is_empty() {
            push_patch_line(&mut old, "");
            push_patch_line(&mut new, "");
        } else {
            return Err(FileEditError::new(
                "malformed_patch",
                "update hunk lines must start with space, -, +, or @@",
                json!({ "line": line }),
            ));
        }
        index += 1;
    }
    flush_hunk(&mut hunks, &mut old, &mut new);
    if hunks.is_empty() && move_to.is_none() {
        return Err(FileEditError::new(
            "malformed_patch",
            "update operation must contain a hunk or move destination",
            json!({}),
        ));
    }
    Ok((hunks, move_to, index))
}

fn flush_hunk(hunks: &mut Vec<PatchHunk>, old: &mut String, new: &mut String) {
    if old.is_empty() && new.is_empty() {
        return;
    }
    hunks.push(PatchHunk {
        old: std::mem::take(old),
        new: std::mem::take(new),
    });
}

fn is_operation_line(line: &str) -> bool {
    line.starts_with("*** Add File: ")
        || line.starts_with("*** Update File: ")
        || line.starts_with("*** Delete File: ")
        || line.starts_with("*** Move File: ")
}

fn push_patch_line(target: &mut String, line: &str) {
    target.push_str(line);
    target.push('\n');
}

fn trim_final_newline(target: &mut String) {
    if target.ends_with('\n') {
        target.pop();
    }
}

async fn ensure_path_absent(
    path: &Path,
    staged: &BTreeMap<PathBuf, StagedFile>,
) -> Result<(), FileEditError> {
    match staged.get(path) {
        Some(StagedFile::Delete) => Ok(()),
        Some(StagedFile::Write(_)) => Err(FileEditError::new(
            "path_exists",
            "target path already exists in staged patch",
            json!({ "path": path_string(path) }),
        )),
        None => match tokio::fs::metadata(path).await {
            Ok(metadata) if metadata.is_dir() => Err(FileEditError::new(
                "target_is_directory",
                "target path is a directory",
                json!({ "path": path_string(path) }),
            )),
            Ok(_) => Err(FileEditError::new(
                "path_exists",
                "target path already exists",
                json!({ "path": path_string(path) }),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(FileEditError::io("metadata_failed", path, error)),
        },
    }
}

async fn ensure_path_exists(
    path: &Path,
    staged: &BTreeMap<PathBuf, StagedFile>,
) -> Result<(), FileEditError> {
    match staged.get(path) {
        Some(StagedFile::Write(_)) => Ok(()),
        Some(StagedFile::Delete) => Err(FileEditError::new(
            "path_missing",
            "target path has already been deleted in staged patch",
            json!({ "path": path_string(path) }),
        )),
        None => match tokio::fs::metadata(path).await {
            Ok(metadata) if metadata.is_dir() => Err(FileEditError::new(
                "target_is_directory",
                "target path is a directory",
                json!({ "path": path_string(path) }),
            )),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(FileEditError::new(
                "path_missing",
                "target path does not exist",
                json!({ "path": path_string(path) }),
            )),
            Err(error) => Err(FileEditError::io("metadata_failed", path, error)),
        },
    }
}

async fn ensure_not_directory(path: &Path) -> Result<(), FileEditError> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) if metadata.is_dir() => Err(FileEditError::new(
            "target_is_directory",
            "target path is a directory",
            json!({ "path": path_string(path) }),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(FileEditError::io("metadata_failed", path, error)),
    }
}

async fn read_staged_text(
    path: &Path,
    staged: &BTreeMap<PathBuf, StagedFile>,
) -> Result<String, FileEditError> {
    let bytes = read_staged_bytes(path, staged).await?;
    String::from_utf8(bytes).map_err(|error| {
        FileEditError::new(
            "invalid_utf8",
            error.to_string(),
            json!({ "path": path_string(path) }),
        )
    })
}

async fn read_required_text(path: &Path) -> Result<String, FileEditError> {
    let bytes = read_existing_file(path).await?;
    String::from_utf8(bytes).map_err(|error| {
        FileEditError::new(
            "invalid_utf8",
            error.to_string(),
            json!({ "path": path_string(path) }),
        )
    })
}

fn replace_string(
    current: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
    path: &Path,
) -> Result<(String, usize), FileEditError> {
    if old_string.is_empty() {
        return Err(FileEditError::new(
            "invalid_input",
            "oldString must not be empty",
            json!({ "path": path_string(path) }),
        ));
    }
    let matches = current.match_indices(old_string).count();
    if matches == 0 {
        return Err(FileEditError::new(
            "replace_no_match",
            "oldString was not found",
            json!({ "path": path_string(path) }),
        ));
    }
    if !replace_all && matches != 1 {
        return Err(FileEditError::new(
            "replace_ambiguous",
            "oldString matched more than once; set replaceAll to true to replace all matches",
            json!({
                "path": path_string(path),
                "matches": matches,
            }),
        ));
    }
    let updated = if replace_all {
        current.replace(old_string, new_string)
    } else {
        current.replacen(old_string, new_string, 1)
    };
    Ok((updated, if replace_all { matches } else { 1 }))
}

async fn read_staged_bytes(
    path: &Path,
    staged: &BTreeMap<PathBuf, StagedFile>,
) -> Result<Vec<u8>, FileEditError> {
    match staged.get(path) {
        Some(StagedFile::Write(content)) => Ok(content.clone()),
        Some(StagedFile::Delete) => Err(FileEditError::new(
            "path_missing",
            "target path has already been deleted in staged patch",
            json!({ "path": path_string(path) }),
        )),
        None => read_existing_file(path).await,
    }
}

async fn read_existing_file(path: &Path) -> Result<Vec<u8>, FileEditError> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(FileEditError::new(
            "path_missing",
            "target path does not exist",
            json!({ "path": path_string(path) }),
        )),
        Err(error) => match tokio::fs::metadata(path).await {
            Ok(metadata) if metadata.is_dir() => Err(FileEditError::new(
                "target_is_directory",
                "target path is a directory",
                json!({ "path": path_string(path) }),
            )),
            _ => Err(FileEditError::io("read_failed", path, error)),
        },
    }
}

fn apply_hunk(current: &str, hunk: &PatchHunk, path: &Path) -> Result<String, FileEditError> {
    if hunk.old.is_empty() {
        return Err(FileEditError::new(
            "patch_context_empty",
            "update hunk must include context or removed lines",
            json!({ "path": path_string(path) }),
        ));
    }
    let mut matches = current.match_indices(&hunk.old);
    let first = matches.next();
    let second = matches.next();
    if first.is_none() || second.is_some() {
        return Err(FileEditError::new(
            "patch_context_mismatch",
            "update hunk context must match exactly once",
            json!({
                "path": path_string(path),
                "matches": if first.is_none() { 0 } else { 2 },
            }),
        ));
    }
    let (start, _) = first.expect("first match exists");
    let end = start + hunk.old.len();
    let mut updated = String::with_capacity(current.len() - hunk.old.len() + hunk.new.len());
    updated.push_str(&current[..start]);
    updated.push_str(&hunk.new);
    updated.push_str(&current[end..]);
    Ok(updated)
}

async fn create_parent_dirs(path: &Path) -> Result<bool, FileEditError> {
    let Some(parent) = path.parent() else {
        return Ok(false);
    };
    match tokio::fs::metadata(parent).await {
        Ok(metadata) if metadata.is_dir() => Ok(false),
        Ok(_) => Err(FileEditError::new(
            "parent_not_directory",
            "parent path exists but is not a directory",
            json!({ "path": path_string(parent) }),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| FileEditError::io("create_parent_failed", parent, error))?;
            Ok(true)
        }
        Err(error) => Err(FileEditError::io("metadata_failed", parent, error)),
    }
}

fn reject_sensitive_path(path: &Path) -> Result<(), FileEditError> {
    if let Some(rule) = sensitive_path_rules()
        .iter()
        .find(|rule| rule.matches(path))
    {
        return Err(FileEditError::new(
            "sensitive_path",
            "refusing to edit a sensitive system path",
            json!({
                "path": path_string(path),
                "rule": rule.as_json(),
            }),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SensitivePathRule {
    Prefix(&'static str),
    Exact(&'static str),
}

impl SensitivePathRule {
    fn matches(self, path: &Path) -> bool {
        match self {
            Self::Prefix(prefix) => path.starts_with(prefix),
            Self::Exact(exact) => path == Path::new(exact),
        }
    }

    fn as_json(self) -> Value {
        match self {
            Self::Prefix(path) => json!({ "kind": "prefix", "path": path }),
            Self::Exact(path) => json!({ "kind": "exact", "path": path }),
        }
    }
}

fn sensitive_path_rules() -> &'static [SensitivePathRule] {
    use SensitivePathRule::{Exact, Prefix};
    &[
        Prefix("/etc"),
        Prefix("/boot"),
        Prefix("/lib/systemd"),
        Prefix("/usr/lib/systemd"),
        Prefix("/run/systemd"),
        Prefix("/System"),
        Prefix("/Library"),
        Prefix("/private/etc"),
        Prefix("/private/var/db"),
        Exact("/var/run/docker.sock"),
        Exact("/run/docker.sock"),
        Exact("/private/var/run/docker.sock"),
    ]
}

fn canonical_guard_path(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut missing = Vec::new();
    while !existing.exists() {
        let Some(parent) = existing.parent() else {
            return normalize_path(path.to_path_buf());
        };
        if let Some(name) = existing.file_name() {
            missing.push(name.to_owned());
        }
        existing = parent;
    }
    let mut canonical =
        std::fs::canonicalize(existing).unwrap_or_else(|_| normalize_path(existing.to_path_buf()));
    for component in missing.iter().rev() {
        canonical.push(component);
    }
    normalize_path(canonical)
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push("..");
                }
            }
            Component::Normal(name) => normalized.push(name),
        }
    }
    normalized
}

fn sorted_unique_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn resolved_path_json(path: &ResolvedPath) -> Value {
    json!({
        "path": path.requested,
        "resolvedPath": path_string(&path.resolved),
    })
}

fn patch_operation_summaries(document: &PatchDocument) -> Vec<Value> {
    document.operation_summaries()
}

impl From<FileEditError> for AgentCoreError {
    fn from(error: FileEditError) -> Self {
        Self::Provider(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_patch_supports_add_update_delete_and_move() {
        let patch = "*** Begin Patch\n*** Add File: a.txt\n+hello\n*** Update File: b.txt\n@@\n-old\n+new\n*** Delete File: c.txt\n*** Move File: d.txt -> e.txt\n*** End Patch";

        let document = parse_patch(patch).expect("patch parses");

        assert_eq!(document.operations.len(), 4);
    }

    #[test]
    fn sorted_unique_paths_orders_lock_keys() {
        let paths = sorted_unique_paths([
            PathBuf::from("/tmp/z"),
            PathBuf::from("/tmp/a"),
            PathBuf::from("/tmp/z"),
        ]);

        assert_eq!(
            paths,
            vec![PathBuf::from("/tmp/a"), PathBuf::from("/tmp/z")]
        );
    }

    #[test]
    fn sensitive_paths_are_rejected() {
        assert!(reject_sensitive_path(Path::new("/etc/passwd")).is_err());
        assert!(reject_sensitive_path(Path::new("/var/run/docker.sock")).is_err());
        assert!(reject_sensitive_path(Path::new("/tmp/project/etc/passwd")).is_ok());
    }
}

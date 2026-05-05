use crate::{
    Catalog, Locale,
    i18n::{ToolOutputOverflowFailureRender, ToolOutputOverflowRender},
    text,
};
use noloong_agent_core::{
    AfterToolCallContext, AfterToolCallResult, BoxFuture, CancellationToken, ContentBlock,
    ToolCallHook, ToolOutput,
};
use serde_json::json;
use std::path::PathBuf;

pub const DEFAULT_MAX_INLINE_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
pub const DEFAULT_TOOL_OUTPUT_PREVIEW_EDGE_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolOutputOverflowConfig {
    pub max_inline_bytes: usize,
    pub preview_head_bytes: usize,
    pub preview_tail_bytes: usize,
    pub temp_dir: PathBuf,
}

impl Default for ToolOutputOverflowConfig {
    fn default() -> Self {
        Self {
            max_inline_bytes: DEFAULT_MAX_INLINE_TOOL_OUTPUT_BYTES,
            preview_head_bytes: DEFAULT_TOOL_OUTPUT_PREVIEW_EDGE_BYTES,
            preview_tail_bytes: DEFAULT_TOOL_OUTPUT_PREVIEW_EDGE_BYTES,
            temp_dir: std::env::temp_dir()
                .join("noloong-agent")
                .join("tool-output"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BuiltInToolOutputOverflowHook {
    config: ToolOutputOverflowConfig,
    catalog: Catalog,
}

impl BuiltInToolOutputOverflowHook {
    pub fn new(config: ToolOutputOverflowConfig) -> Self {
        Self {
            config,
            catalog: Catalog::new(Locale::En),
        }
    }

    pub fn with_catalog(mut self, catalog: Catalog) -> Self {
        self.catalog = catalog;
        self
    }

    pub fn config(&self) -> &ToolOutputOverflowConfig {
        &self.config
    }
}

impl ToolCallHook for BuiltInToolOutputOverflowHook {
    fn id(&self) -> Option<&str> {
        Some("noloong.builtin.tool-output-overflow")
    }

    fn after_tool_call<'a>(
        &'a self,
        context: AfterToolCallContext,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, Option<AfterToolCallResult>> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let serialized = match serde_json::to_vec(&context.output) {
                Ok(serialized) => serialized,
                Err(error) => {
                    return Ok(Some(failure_result(
                        &context,
                        self.config.max_inline_bytes,
                        None,
                        self.catalog.failed_to_serialize_tool_output(error),
                        &self.catalog,
                    )));
                }
            };
            let original_bytes = serialized.len();
            if original_bytes <= self.config.max_inline_bytes {
                return Ok(None);
            }

            let path = overflow_path(&self.config.temp_dir, &context);
            cancellation.throw_if_cancelled()?;
            let result = match tokio::fs::create_dir_all(&self.config.temp_dir).await {
                Ok(()) => tokio::fs::write(&path, &serialized).await,
                Err(error) => Err(error),
            };
            match result {
                Ok(()) => Ok(Some(overflow_result(
                    &context,
                    path,
                    original_bytes,
                    self.config.max_inline_bytes,
                    output_preview(&context.output, &serialized, preview_edges(&self.config)),
                    &self.catalog,
                ))),
                Err(error) => Ok(Some(failure_result(
                    &context,
                    self.config.max_inline_bytes,
                    Some(original_bytes),
                    self.catalog.failed_to_persist_tool_output(error),
                    &self.catalog,
                ))),
            }
        })
    }
}

fn overflow_result(
    context: &AfterToolCallContext,
    path: PathBuf,
    original_bytes: usize,
    inline_limit_bytes: usize,
    preview: OutputPreview,
    catalog: &Catalog,
) -> AfterToolCallResult {
    let path_text = path.display().to_string();
    let text = catalog.render_tool_output_overflow(ToolOutputOverflowRender {
        path: &path,
        tool_name: &context.tool_call.name,
        tool_call_id: &context.tool_call.id,
        original_bytes,
        inline_limit_bytes,
        preview_head: &preview.head,
        preview_tail: &preview.tail,
        preview_omitted_bytes: preview.omitted_bytes,
    });
    AfterToolCallResult {
        content: Some(vec![ContentBlock::Text { text }]),
        details: Some(json!({
            "overflow": true,
            "path": path_text,
            "originalBytes": original_bytes,
            "inlineLimitBytes": inline_limit_bytes,
            "toolName": context.tool_call.name,
            "toolCallId": context.tool_call.id,
            "previewHead": preview.head,
            "previewTail": preview.tail,
            "previewOmittedBytes": preview.omitted_bytes,
        })),
        is_error: Some(false),
    }
}

fn failure_result(
    context: &AfterToolCallContext,
    inline_limit_bytes: usize,
    original_bytes: Option<usize>,
    error: String,
    catalog: &Catalog,
) -> AfterToolCallResult {
    let text = catalog.render_tool_output_overflow_failure(ToolOutputOverflowFailureRender {
        tool_name: &context.tool_call.name,
        tool_call_id: &context.tool_call.id,
        inline_limit_bytes,
        error: &error,
    });
    AfterToolCallResult {
        content: Some(vec![ContentBlock::Text { text }]),
        details: Some(json!({
            "overflow": true,
            "persistenceFailed": true,
            "originalBytes": original_bytes,
            "inlineLimitBytes": inline_limit_bytes,
            "toolName": context.tool_call.name,
            "toolCallId": context.tool_call.id,
            "error": error,
        })),
        is_error: Some(true),
    }
}

fn overflow_path(root: &std::path::Path, context: &AfterToolCallContext) -> PathBuf {
    let filename = format!(
        "{}-{}-{}.json",
        safe_path_component(&context.run_id),
        context.turn_id,
        safe_path_component(&context.tool_call.id)
    );
    root.join(filename)
}

fn safe_path_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OutputPreview {
    head: String,
    tail: String,
    omitted_bytes: usize,
}

fn preview_edges(config: &ToolOutputOverflowConfig) -> (usize, usize) {
    let edge_cap = config.max_inline_bytes / 4;
    (
        config.preview_head_bytes.min(edge_cap),
        config.preview_tail_bytes.min(edge_cap),
    )
}

fn output_preview(
    output: &ToolOutput,
    serialized: &[u8],
    (head_bytes, tail_bytes): (usize, usize),
) -> OutputPreview {
    let mut builder = OutputPreviewBuilder::new(head_bytes, tail_bytes);
    if output.content.is_empty() {
        builder.push_str(&String::from_utf8_lossy(serialized));
    } else {
        for (index, content) in output.content.iter().enumerate() {
            if index > 0 {
                builder.push_str("\n");
            }
            push_content_preview(&mut builder, content);
        }
    }
    builder.finish()
}

fn push_content_preview(builder: &mut OutputPreviewBuilder, content: &ContentBlock) {
    match content {
        ContentBlock::Text { text } => builder.push_str(text),
        ContentBlock::Json { value } => {
            if let Ok(text) = serde_json::to_string(value) {
                builder.push_str(&text);
            }
        }
        other => {
            if let Ok(text) = serde_json::to_string(other) {
                builder.push_str(&text);
            }
        }
    }
}

#[derive(Debug)]
struct OutputPreviewBuilder {
    head: String,
    tail: String,
    full: Option<String>,
    total_bytes: usize,
    head_bytes: usize,
    tail_bytes: usize,
}

impl OutputPreviewBuilder {
    fn new(head_bytes: usize, tail_bytes: usize) -> Self {
        Self {
            head: String::new(),
            tail: String::new(),
            full: Some(String::new()),
            total_bytes: 0,
            head_bytes,
            tail_bytes,
        }
    }

    fn push_str(&mut self, text: &str) {
        self.total_bytes += text.len();
        self.push_full(text);
        self.push_head(text);
        self.push_tail(text);
    }

    fn finish(self) -> OutputPreview {
        if let Some(full) = self.full {
            return OutputPreview {
                head: full,
                tail: String::new(),
                omitted_bytes: 0,
            };
        }
        OutputPreview {
            omitted_bytes: self
                .total_bytes
                .saturating_sub(self.head.len())
                .saturating_sub(self.tail.len()),
            head: self.head,
            tail: self.tail,
        }
    }

    fn push_full(&mut self, text: &str) {
        let Some(full) = self.full.as_mut() else {
            return;
        };
        if full.len().saturating_add(text.len()) <= self.head_bytes.saturating_add(self.tail_bytes)
        {
            full.push_str(text);
        } else {
            self.full = None;
        }
    }

    fn push_head(&mut self, text: &str) {
        let remaining = self.head_bytes.saturating_sub(self.head.len());
        if remaining == 0 {
            return;
        }
        self.head.push_str(&text::prefix_to_bytes(text, remaining));
    }

    fn push_tail(&mut self, text: &str) {
        if self.tail_bytes == 0 {
            return;
        }
        if text.len() >= self.tail_bytes {
            self.tail = text::suffix_to_bytes(text, self.tail_bytes);
            return;
        }
        self.tail.push_str(text);
        self.tail = text::suffix_to_bytes(&self.tail, self.tail_bytes);
    }
}

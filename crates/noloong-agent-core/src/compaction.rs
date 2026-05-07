use crate::{
    AgentCoreError, AgentMessage, CancellationToken, ContentBlock, MediaSource, MessageRole,
    ModelProvider, ModelRequest, ModelStreamEvent, Result, StopReason, ToolCall,
    provider_utils::collect_model_stream, providers::BoxFuture,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub const COMPACTION_METADATA_KEY: &str = "noloong.compaction";
pub(crate) const COMPACTION_METADATA_IS_SPLIT_TURN_KEY: &str = "isSplitTurn";
pub(crate) const COMPACTION_METADATA_MODE_KEY: &str = "mode";
pub(crate) const COMPACTION_METADATA_TOKENS_BEFORE_KEY: &str = "tokensBefore";
const DEFAULT_RESERVE_TOKENS: u64 = 16_384;
const DEFAULT_KEEP_RECENT_TOKENS: u64 = 20_000;
const TOOL_RESULT_SUMMARY_MAX_CHARS: usize = 2_000;
const MEDIA_TOKEN_ESTIMATE: u64 = 1_200;

pub trait CompactionSummarizer: Send + Sync {
    fn id(&self) -> &str;

    fn summarize<'a>(
        &'a self,
        request: CompactionSummaryRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, CompactionSummaryResult>;
}

pub trait ContextCompactor: Send + Sync {
    fn id(&self) -> &str;

    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ContextCompactionOutput>;
}

pub trait TokenEstimator: Send + Sync {
    fn estimate_message_tokens(&self, message: &AgentMessage) -> u64;

    fn estimate_messages_tokens(&self, messages: &[AgentMessage]) -> u64 {
        messages
            .iter()
            .map(|message| self.estimate_message_tokens(message))
            .sum()
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ContextCompactionMode {
    #[default]
    PersistentState,
    RequestOnly,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompactionConfig {
    pub context_window_tokens: u64,
    pub reserve_tokens: u64,
    pub keep_recent_tokens: u64,
    pub mode: ContextCompactionMode,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl ContextCompactionConfig {
    pub fn new(context_window_tokens: u64) -> Self {
        Self {
            context_window_tokens,
            reserve_tokens: DEFAULT_RESERVE_TOKENS,
            keep_recent_tokens: DEFAULT_KEEP_RECENT_TOKENS,
            mode: ContextCompactionMode::PersistentState,
            metadata: Map::new(),
        }
    }

    pub fn reserve_tokens(mut self, reserve_tokens: u64) -> Self {
        self.reserve_tokens = reserve_tokens;
        self
    }

    pub fn keep_recent_tokens(mut self, keep_recent_tokens: u64) -> Self {
        self.keep_recent_tokens = keep_recent_tokens;
        self
    }

    pub fn mode(mut self, mode: ContextCompactionMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.context_window_tokens == 0 {
            return Err(AgentCoreError::InvalidEffect(
                "compaction context window must be greater than zero".into(),
            ));
        }
        if self.reserve_tokens >= self.context_window_tokens {
            return Err(AgentCoreError::InvalidEffect(
                "compaction reserve tokens must be smaller than context window".into(),
            ));
        }
        if self.keep_recent_tokens == 0 {
            return Err(AgentCoreError::InvalidEffect(
                "compaction keep recent tokens must be greater than zero".into(),
            ));
        }
        Ok(())
    }

    pub fn trigger_threshold(&self) -> u64 {
        self.context_window_tokens - self.reserve_tokens
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummaryRequest {
    pub run_id: String,
    pub turn_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_summary: Option<String>,
    #[serde(default)]
    pub messages_to_summarize: Vec<AgentMessage>,
    #[serde(default)]
    pub turn_prefix_messages: Vec<AgentMessage>,
    pub token_budget: u64,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummaryResult {
    pub summary: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompactionRequest {
    pub run_id: String,
    pub turn_id: u64,
    #[serde(default)]
    pub current_messages: Vec<AgentMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_summary: Option<String>,
    #[serde(default)]
    pub messages_to_summarize: Vec<AgentMessage>,
    #[serde(default)]
    pub turn_prefix_messages: Vec<AgentMessage>,
    #[serde(default)]
    pub retained_messages: Vec<AgentMessage>,
    pub token_budget: u64,
    pub tokens_before: u64,
    pub estimated_retained_tokens: u64,
    pub is_split_turn: bool,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl ContextCompactionRequest {
    fn into_summary_request(self) -> CompactionSummaryRequest {
        CompactionSummaryRequest {
            run_id: self.run_id,
            turn_id: self.turn_id,
            previous_summary: self.previous_summary,
            messages_to_summarize: self.messages_to_summarize,
            turn_prefix_messages: self.turn_prefix_messages,
            token_budget: self.token_budget,
            metadata: self.metadata,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "result", rename_all = "snake_case")]
pub enum ContextCompactionOutput {
    Summary(CompactionSummaryResult),
    Replacement(CompactionReplacementResult),
}

impl ContextCompactionOutput {
    pub fn summary(summary: impl Into<String>) -> Self {
        Self::Summary(CompactionSummaryResult {
            summary: summary.into(),
            metadata: Map::new(),
        })
    }

    pub fn replacement(messages: Vec<AgentMessage>) -> Self {
        Self::Replacement(CompactionReplacementResult {
            replacement_messages: messages,
            metadata: Map::new(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompactionReplacementResult {
    #[serde(default)]
    pub replacement_messages: Vec<AgentMessage>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Clone)]
pub struct SummaryContextCompactor {
    summarizer: Arc<dyn CompactionSummarizer>,
}

impl SummaryContextCompactor {
    pub fn new(summarizer: Arc<dyn CompactionSummarizer>) -> Self {
        Self { summarizer }
    }
}

impl ContextCompactor for SummaryContextCompactor {
    fn id(&self) -> &str {
        self.summarizer.id()
    }

    fn compact<'a>(
        &'a self,
        request: ContextCompactionRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ContextCompactionOutput> {
        Box::pin(async move {
            let result = self
                .summarizer
                .summarize(request.into_summary_request(), cancellation)
                .await?;
            Ok(ContextCompactionOutput::Summary(result))
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompactionDecision {
    Skip { estimated_tokens: u64 },
    Compact(CompactionPlan),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactionPlan {
    pub previous_summary: Option<String>,
    pub messages_to_summarize: Vec<AgentMessage>,
    pub turn_prefix_messages: Vec<AgentMessage>,
    pub retained_messages: Vec<AgentMessage>,
    pub tokens_before: u64,
    pub estimated_retained_tokens: u64,
    pub is_split_turn: bool,
    retained_message_ids: Vec<String>,
    dropped_message_ids: Vec<String>,
}

impl CompactionPlan {
    pub fn retained_message_ids(&self) -> &[String] {
        &self.retained_message_ids
    }

    pub fn dropped_message_ids(&self) -> &[String] {
        &self.dropped_message_ids
    }
}

#[derive(Clone, Debug, Default)]
pub struct HeuristicTokenEstimator;

impl TokenEstimator for HeuristicTokenEstimator {
    fn estimate_message_tokens(&self, message: &AgentMessage) -> u64 {
        estimate_chars_as_tokens(
            message.role.as_str().len() + estimate_content_chars(&message.content),
        )
    }
}

#[derive(Clone)]
pub struct ModelBackedCompactionSummarizer {
    config: ModelBackedCompactionSummarizerConfig,
    provider: Arc<dyn ModelProvider>,
}

impl ModelBackedCompactionSummarizer {
    pub fn new(
        config: ModelBackedCompactionSummarizerConfig,
        provider: Arc<dyn ModelProvider>,
    ) -> Self {
        Self { config, provider }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelBackedCompactionSummarizerConfig {
    pub id: String,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl ModelBackedCompactionSummarizerConfig {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            metadata: Map::new(),
        }
    }

    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

impl CompactionSummarizer for ModelBackedCompactionSummarizer {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn summarize<'a>(
        &'a self,
        request: CompactionSummaryRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, CompactionSummaryResult> {
        Box::pin(async move {
            let mut metadata = self.config.metadata.clone();
            metadata.extend(request.metadata.clone());

            let history_summary = if request.messages_to_summarize.is_empty() {
                request
                    .previous_summary
                    .clone()
                    .unwrap_or_else(|| "No prior history.".into())
            } else {
                let prompt = history_summary_prompt(&request);
                self.summarize_prompt(prompt, &request, cancellation.clone())
                    .await?
            };

            let summary = if request.turn_prefix_messages.is_empty() {
                history_summary
            } else {
                let prompt = turn_prefix_summary_prompt(&request.turn_prefix_messages);
                let turn_prefix = self
                    .summarize_prompt(prompt, &request, cancellation.clone())
                    .await?;
                format!(
                    "{history_summary}\n\n---\n\n**Turn Context (split turn):**\n\n{turn_prefix}"
                )
            };

            if summary.trim().is_empty() {
                return Err(AgentCoreError::Provider(
                    "compaction summarizer returned an empty summary".into(),
                ));
            }
            Ok(CompactionSummaryResult { summary, metadata })
        })
    }
}

impl ModelBackedCompactionSummarizer {
    async fn summarize_prompt(
        &self,
        prompt: String,
        request: &CompactionSummaryRequest,
        cancellation: CancellationToken,
    ) -> Result<String> {
        cancellation.throw_if_cancelled()?;
        let model_request = ModelRequest {
            run_id: request.run_id.clone(),
            turn_id: request.turn_id,
            messages: vec![
                AgentMessage {
                    id: format!("compaction-system-{}-{}", request.run_id, request.turn_id),
                    role: MessageRole::System,
                    content: vec![ContentBlock::Text {
                        text: SUMMARIZATION_SYSTEM_PROMPT.into(),
                    }],
                    metadata: Map::new(),
                },
                AgentMessage::user(
                    format!("compaction-user-{}-{}", request.run_id, request.turn_id),
                    prompt,
                ),
            ],
            context: Map::new(),
            tools: Vec::new(),
            metadata: request.metadata.clone(),
        };
        let stream =
            collect_model_stream(self.provider.as_ref(), model_request, None, cancellation).await?;
        collect_text_summary(&stream.events)
    }
}

pub fn plan_compaction(
    config: &ContextCompactionConfig,
    estimator: &dyn TokenEstimator,
    messages: &[AgentMessage],
) -> Result<CompactionDecision> {
    config.validate()?;
    if messages.is_empty() {
        return Ok(CompactionDecision::Skip {
            estimated_tokens: 0,
        });
    }
    let token_estimates = messages
        .iter()
        .map(|message| estimator.estimate_message_tokens(message))
        .collect::<Vec<_>>();
    let tokens_before = token_estimates.iter().sum();
    if tokens_before <= config.trigger_threshold() {
        return Ok(CompactionDecision::Skip {
            estimated_tokens: tokens_before,
        });
    }

    let previous_summary_index = find_previous_compaction_summary(messages);
    let previous_summary =
        previous_summary_index.and_then(|index| message_visible_text(&messages[index]));
    let boundary_start = previous_summary_index.map_or(0, |index| index + 1);
    if boundary_start >= messages.len() {
        return Ok(CompactionDecision::Skip {
            estimated_tokens: tokens_before,
        });
    }

    let cut_point = find_cut_point(
        messages,
        &token_estimates,
        boundary_start,
        messages.len(),
        config.keep_recent_tokens,
    );
    let Some(cut_point) = cut_point else {
        return Ok(CompactionDecision::Skip {
            estimated_tokens: tokens_before,
        });
    };
    if cut_point.first_kept_index == 0 {
        return Ok(CompactionDecision::Skip {
            estimated_tokens: tokens_before,
        });
    }

    let history_end = if cut_point.is_split_turn {
        cut_point.turn_start_index
    } else {
        cut_point.first_kept_index
    };
    let messages_to_summarize = messages[boundary_start..history_end]
        .iter()
        .filter(|message| !is_compaction_summary(message))
        .cloned()
        .collect::<Vec<_>>();
    let turn_prefix_messages = if cut_point.is_split_turn {
        messages[cut_point.turn_start_index..cut_point.first_kept_index].to_vec()
    } else {
        Vec::new()
    };
    if messages_to_summarize.is_empty()
        && turn_prefix_messages.is_empty()
        && previous_summary.is_none()
    {
        return Ok(CompactionDecision::Skip {
            estimated_tokens: tokens_before,
        });
    }

    let retained_messages = messages[cut_point.first_kept_index..].to_vec();
    let retained_message_ids = retained_messages
        .iter()
        .map(|message| message.id.clone())
        .collect();
    let dropped_message_ids = messages[..cut_point.first_kept_index]
        .iter()
        .map(|message| message.id.clone())
        .collect();
    let estimated_retained_tokens = token_estimates[cut_point.first_kept_index..].iter().sum();

    Ok(CompactionDecision::Compact(CompactionPlan {
        previous_summary,
        messages_to_summarize,
        turn_prefix_messages,
        retained_messages,
        tokens_before,
        estimated_retained_tokens,
        is_split_turn: cut_point.is_split_turn,
        retained_message_ids,
        dropped_message_ids,
    }))
}

pub fn compaction_summary_message(
    run_id: &str,
    turn_id: u64,
    summary: String,
    mut metadata: Map<String, Value>,
) -> AgentMessage {
    metadata.insert(
        COMPACTION_METADATA_KEY.into(),
        json!({
            "runId": run_id,
            "turnId": turn_id,
        }),
    );
    AgentMessage {
        id: format!("compaction-summary-{run_id}-{turn_id}"),
        role: MessageRole::System,
        content: vec![ContentBlock::Text { text: summary }],
        metadata,
    }
}

pub fn compacted_messages(
    summary: AgentMessage,
    retained_messages: &[AgentMessage],
) -> Vec<AgentMessage> {
    let mut messages = Vec::with_capacity(retained_messages.len() + 1);
    messages.push(summary);
    messages.extend_from_slice(retained_messages);
    messages
}

pub fn serialize_messages_for_summary(messages: &[AgentMessage]) -> String {
    messages
        .iter()
        .filter_map(serialize_message_for_summary)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn collect_text_summary(events: &[ModelStreamEvent]) -> Result<String> {
    let mut text = String::new();
    for event in events {
        match event {
            ModelStreamEvent::TextDelta { text: delta } => text.push_str(delta),
            ModelStreamEvent::Failed { error } => {
                return Err(AgentCoreError::Provider(format!(
                    "compaction summarizer failed: {error}"
                )));
            }
            ModelStreamEvent::Finished {
                stop_reason: StopReason::Error | StopReason::Aborted,
            } => {
                return Err(AgentCoreError::Provider(
                    "compaction summarizer finished unsuccessfully".into(),
                ));
            }
            _ => {}
        }
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AgentCoreError::Provider(
            "compaction summarizer returned no text".into(),
        ));
    }
    Ok(text)
}

fn history_summary_prompt(request: &CompactionSummaryRequest) -> String {
    let conversation = serialize_messages_for_summary(&request.messages_to_summarize);
    if let Some(previous_summary) = &request.previous_summary {
        format!(
            "<previous-summary>\n{previous_summary}\n</previous-summary>\n\n<conversation>\n{conversation}\n</conversation>\n\n{UPDATE_SUMMARIZATION_PROMPT}"
        )
    } else {
        format!("<conversation>\n{conversation}\n</conversation>\n\n{SUMMARIZATION_PROMPT}")
    }
}

fn turn_prefix_summary_prompt(messages: &[AgentMessage]) -> String {
    let conversation = serialize_messages_for_summary(messages);
    format!("<conversation>\n{conversation}\n</conversation>\n\n{TURN_PREFIX_SUMMARIZATION_PROMPT}")
}

fn find_previous_compaction_summary(messages: &[AgentMessage]) -> Option<usize> {
    messages.iter().rposition(is_compaction_summary)
}

fn is_compaction_summary(message: &AgentMessage) -> bool {
    matches!(message.role, MessageRole::System)
        && message.metadata.contains_key(COMPACTION_METADATA_KEY)
}

fn message_visible_text(message: &AgentMessage) -> Option<String> {
    let text = message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.trim().is_empty()).then_some(text)
}

struct CutPoint {
    first_kept_index: usize,
    turn_start_index: usize,
    is_split_turn: bool,
}

fn find_cut_point(
    messages: &[AgentMessage],
    token_estimates: &[u64],
    start_index: usize,
    end_index: usize,
    keep_recent_tokens: u64,
) -> Option<CutPoint> {
    let first_valid_cut =
        (start_index..end_index).find(|index| is_valid_cut_role(&messages[*index].role))?;

    let mut accumulated_tokens = 0;
    let mut target_index = None;
    for index in (start_index..end_index).rev() {
        accumulated_tokens += token_estimates[index];
        if accumulated_tokens >= keep_recent_tokens {
            target_index = Some(index);
            break;
        }
    }
    let cut_index = target_index
        .and_then(|index| {
            (index..end_index).find(|candidate| is_valid_cut_role(&messages[*candidate].role))
        })
        .unwrap_or(first_valid_cut);

    let is_user_message = matches!(messages[cut_index].role, MessageRole::User);
    let turn_start_index = if is_user_message {
        cut_index
    } else {
        find_turn_start(messages, cut_index, start_index)?
    };
    Some(CutPoint {
        first_kept_index: cut_index,
        turn_start_index,
        is_split_turn: !is_user_message,
    })
}

fn is_valid_cut_role(role: &MessageRole) -> bool {
    matches!(role, MessageRole::User | MessageRole::Assistant)
}

fn find_turn_start(
    messages: &[AgentMessage],
    entry_index: usize,
    start_index: usize,
) -> Option<usize> {
    (start_index..=entry_index)
        .rev()
        .find(|index| matches!(messages[*index].role, MessageRole::User))
}

fn serialize_message_for_summary(message: &AgentMessage) -> Option<String> {
    let content = serialize_content_for_summary(&message.content);
    if content.trim().is_empty() {
        return None;
    }
    Some(format!("[{}]: {content}", message.role.as_str()))
}

fn serialize_content_for_summary(content: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => parts.push(text.clone()),
            ContentBlock::Json { value } => parts.push(value.to_string()),
            ContentBlock::Thinking { thinking } => {
                if let Some(text) = &thinking.text {
                    parts.push(format!("[thinking:{}] {text}", thinking.kind.as_str()));
                } else if let Some(raw) = &thinking.raw {
                    parts.push(format!("[thinking:{}] {raw}", thinking.kind.as_str()));
                }
            }
            ContentBlock::Media { media } => {
                let name = media.name.as_deref().unwrap_or("unnamed");
                let mime = media.mime_type.as_deref().unwrap_or("unknown");
                parts.push(format!(
                    "[media:{} name={name} mime={mime}]",
                    media.kind.as_str()
                ));
            }
            ContentBlock::ToolCall { tool_call } => parts.push(serialize_tool_call(tool_call)),
            ContentBlock::ToolResult {
                tool_call_id,
                tool_name,
                content,
                is_error,
            } => {
                let nested = serialize_content_for_summary(content);
                let nested = truncate_for_summary(&nested, TOOL_RESULT_SUMMARY_MAX_CHARS);
                parts.push(format!(
                    "[tool_result:{tool_name} id={tool_call_id} is_error={is_error}] {nested}"
                ));
            }
            ContentBlock::ProviderPayload {
                provider,
                kind,
                value,
            } => {
                parts.push(format!("[provider_payload:{provider}/{kind}] {value}"));
            }
        }
    }
    parts.join("\n")
}

fn serialize_tool_call(tool_call: &ToolCall) -> String {
    format!(
        "[tool_call:{} id={}] {}",
        tool_call.name, tool_call.id, tool_call.arguments
    )
}

fn estimate_content_chars(content: &[ContentBlock]) -> usize {
    content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::Json { value } => value.to_string().len(),
            ContentBlock::Thinking { thinking } => {
                thinking.text.as_ref().map_or(0, String::len)
                    + thinking.raw.as_ref().map_or(0, |raw| raw.to_string().len())
                    + thinking
                        .replay_descriptor
                        .as_ref()
                        .map_or(0, |descriptor| descriptor.to_string().len())
            }
            ContentBlock::Media { media } => match &media.source {
                MediaSource::Inline { data, .. } => data.len(),
                _ => (MEDIA_TOKEN_ESTIMATE * 4) as usize,
            },
            ContentBlock::ToolCall { tool_call } => {
                tool_call.name.len() + tool_call.arguments.to_string().len()
            }
            ContentBlock::ToolResult { content, .. } => estimate_content_chars(content),
            ContentBlock::ProviderPayload {
                provider,
                kind,
                value,
            } => provider.len() + kind.len() + value.to_string().len(),
        })
        .sum()
}

fn estimate_chars_as_tokens(chars: usize) -> u64 {
    chars.div_ceil(4).max(1) as u64
}

fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    let Some((boundary, _)) = text.char_indices().nth(max_chars) else {
        return text.to_string();
    };
    let omitted = text[boundary..].chars().count();
    format!(
        "{}\n\n[... {omitted} more characters truncated]",
        &text[..boundary]
    )
}

const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Read the conversation and produce only the requested structured summary. Do not continue the conversation.";

const SUMMARIZATION_PROMPT: &str = r#"The messages above are a conversation to summarize. Create a structured context checkpoint summary that another LLM will use to continue the work.

Use this exact format:

## Goal
[What is the user trying to accomplish?]

## Constraints & Preferences
- [Any constraints, preferences, or requirements mentioned by user]

## Progress
### Done
- [x] [Completed tasks/changes]

### In Progress
- [ ] [Current work]

### Blocked
- [Issues preventing progress, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [Ordered list of what should happen next]

## Critical Context
- [Any data, examples, paths, identifiers, or error messages needed to continue]

Keep each section concise. Preserve exact file paths, function names, type names, commands, and error messages."#;

const UPDATE_SUMMARIZATION_PROMPT: &str = r#"The messages above are new conversation messages to incorporate into the existing summary in <previous-summary>.

Update the existing structured summary with new information. Rules:
- Preserve all still-relevant information from the previous summary.
- Add new progress, decisions, constraints, and critical context.
- Move completed in-progress work to Done when appropriate.
- Update Next Steps based on what remains.
- Preserve exact file paths, function names, type names, commands, and error messages.

Use the same structured format as the previous summary."#;

const TURN_PREFIX_SUMMARIZATION_PROMPT: &str = r#"This is the prefix of a turn that was too large to keep. The suffix is retained.

Summarize the prefix to provide context for the retained suffix:

## Original Request
[What did the user ask for in this turn?]

## Early Progress
- [Key decisions and work done in the prefix]

## Context for Suffix
- [Information needed to understand the kept suffix]

Be concise. Focus on what is needed to understand the kept suffix."#;

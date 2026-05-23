use crate::interaction::{
    AppContentBlock, AppDisplayEvent, AppInteractionSessionDescriptor, AppInteractionSessionStatus,
    AppMessage, AppToolApprovalRequest, AppToolOutput,
};
use std::time::{SystemTime, UNIX_EPOCH};

mod composer;
mod streaming;
mod transcript;

pub use composer::{ChatComposer, ChatComposerAction};
pub use streaming::StreamingText;
pub use transcript::{
    ChatApprovalCard, ChatApprovalStatus, ChatToolActivity, ChatTranscriptItem, ChatTranscriptRole,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatSessionStore {
    descriptors: Vec<AppInteractionSessionDescriptor>,
    sessions: Vec<ChatSessionSummary>,
    current_session_id: Option<String>,
    transcript: Vec<ChatTranscriptItem>,
    current_run: Option<ChatRunState>,
    connection_error: Option<String>,
}

impl ChatSessionStore {
    pub fn refresh(&mut self, descriptors: Vec<AppInteractionSessionDescriptor>) {
        self.sessions = descriptors.iter().map(ChatSessionSummary::from).collect();
        if !self.current_session_id.as_ref().is_some_and(|session_id| {
            self.sessions
                .iter()
                .any(|session| &session.session_id == session_id)
        }) {
            self.current_session_id = self
                .sessions
                .first()
                .map(|session| session.session_id.clone());
        }
        self.recover_current_transcript(&descriptors);
        self.recover_current_run(&descriptors);
        self.descriptors = descriptors;
    }

    pub fn select_session(&mut self, session_id: &str) -> bool {
        if !self
            .sessions
            .iter()
            .any(|session| session.session_id == session_id)
        {
            return false;
        }
        self.current_session_id = Some(session_id.into());
        let descriptors = self.descriptors.clone();
        self.recover_current_transcript(&descriptors);
        self.recover_current_run(&descriptors);
        true
    }

    pub fn upsert_and_select(&mut self, descriptor: AppInteractionSessionDescriptor) {
        let session_id = descriptor.session_id.clone();
        let previous_transcript = (self.current_session_id.as_deref() == Some(session_id.as_str()))
            .then(|| self.transcript.clone());
        if let Some(existing) = self
            .descriptors
            .iter_mut()
            .find(|existing| existing.session_id == session_id)
        {
            *existing = descriptor;
        } else {
            self.descriptors.push(descriptor);
        }
        self.sessions = self
            .descriptors
            .iter()
            .map(ChatSessionSummary::from)
            .collect();
        self.current_session_id = Some(session_id);
        let descriptors = self.descriptors.clone();
        self.recover_current_transcript(&descriptors);
        if let Some(previous_transcript) = previous_transcript {
            self.merge_live_display_items(previous_transcript);
        }
        self.recover_current_run(&descriptors);
    }

    pub fn sessions(&self) -> &[ChatSessionSummary] {
        &self.sessions
    }

    pub fn current_session_id(&self) -> Option<&str> {
        self.current_session_id.as_deref()
    }

    pub fn transcript(&self) -> &[ChatTranscriptItem] {
        &self.transcript
    }

    pub fn current_run(&self) -> Option<&ChatRunState> {
        self.current_run.as_ref()
    }

    pub fn connection_error(&self) -> Option<&str> {
        self.connection_error.as_deref()
    }

    pub fn set_connection_error(&mut self, error: String) {
        self.connection_error = Some(error);
    }

    pub fn resolve_approval(&mut self, approval_id: &str, status: ChatApprovalStatus) -> bool {
        let Some(approval) = self
            .transcript
            .iter_mut()
            .find(|item| item.message_id == approval_id)
            .and_then(ChatTranscriptItem::approval_mut)
        else {
            return false;
        };
        approval.resolve(status);
        true
    }

    pub fn update_session_descriptor_preserving_transcript(
        &mut self,
        descriptor: AppInteractionSessionDescriptor,
    ) {
        let session_id = descriptor.session_id.clone();
        if let Some(existing) = self
            .descriptors
            .iter_mut()
            .find(|existing| existing.session_id == session_id)
        {
            *existing = descriptor;
        } else {
            self.descriptors.push(descriptor);
        }
        self.sessions = self
            .descriptors
            .iter()
            .map(ChatSessionSummary::from)
            .collect();
        if self.current_session_id.is_none() {
            self.current_session_id = Some(session_id);
        }
        let descriptors = self.descriptors.clone();
        self.recover_current_run(&descriptors);
    }

    pub fn can_send_current_message(&self) -> bool {
        !matches!(
            self.current_run.as_ref().map(|run| run.status),
            Some(ChatRunStatus::Running | ChatRunStatus::Paused)
        )
    }

    pub fn apply_display_event(&mut self, event: AppDisplayEvent) {
        self.apply_display_event_at(event, now_ms());
    }

    pub fn apply_display_event_at(&mut self, event: AppDisplayEvent, now_ms: u64) {
        match event {
            AppDisplayEvent::RunStarted { run_id } => {
                self.connection_error = None;
                self.set_current_run(
                    ChatRunState {
                        run_id: Some(run_id),
                        status: ChatRunStatus::Running,
                        error: None,
                    },
                    AppInteractionSessionStatus::Running,
                );
            }
            AppDisplayEvent::RunCompleted { run_id } => {
                self.set_current_run(
                    ChatRunState {
                        run_id: Some(run_id),
                        status: ChatRunStatus::Completed,
                        error: None,
                    },
                    AppInteractionSessionStatus::Completed,
                );
            }
            AppDisplayEvent::RunAborted { run_id } => {
                self.set_current_run(
                    ChatRunState {
                        run_id: Some(run_id),
                        status: ChatRunStatus::Aborted,
                        error: None,
                    },
                    AppInteractionSessionStatus::Aborted,
                );
            }
            AppDisplayEvent::RunFailed { run_id, error } => {
                self.set_current_run(
                    ChatRunState {
                        run_id: Some(run_id),
                        status: ChatRunStatus::Failed,
                        error: Some(error),
                    },
                    AppInteractionSessionStatus::Failed,
                );
            }
            AppDisplayEvent::RunPaused { run_id, .. } => {
                self.set_current_run(
                    ChatRunState {
                        run_id: Some(run_id),
                        status: ChatRunStatus::Paused,
                        error: None,
                    },
                    AppInteractionSessionStatus::Paused,
                );
            }
            AppDisplayEvent::AssistantMessageDelta {
                display_message_id,
                text,
                ..
            } => {
                if let Some(item) = self
                    .transcript
                    .iter_mut()
                    .find(|item| item.message_id == display_message_id)
                {
                    item.push_assistant_delta(text, now_ms);
                } else if !text.is_empty() {
                    let mut streaming = StreamingText::default();
                    streaming.push_delta(text, now_ms);
                    self.transcript.push(ChatTranscriptItem::assistant(
                        display_message_id,
                        streaming.text(),
                        Some(streaming),
                    ));
                }
            }
            AppDisplayEvent::AssistantMessageFinal {
                display_message_id,
                message,
                ..
            } => {
                let Some(text) = text_from_message(&message) else {
                    return;
                };
                if let Some(item) = self
                    .transcript
                    .iter_mut()
                    .find(|item| item.message_id == display_message_id)
                {
                    item.replace_with_assistant(message.id, text);
                } else {
                    self.transcript
                        .push(ChatTranscriptItem::assistant(message.id, text, None));
                }
            }
            AppDisplayEvent::ThoughtStarted { thought_id, .. } => {
                self.upsert_thought(&thought_id);
            }
            AppDisplayEvent::ThoughtDelta {
                thought_id,
                kind,
                text,
                ..
            } => {
                let item = self.upsert_thought(&thought_id);
                if let Some(thought) = item.thought_mut() {
                    thought.push_delta(&kind, &text);
                }
            }
            AppDisplayEvent::ThoughtCompleted {
                thought_id,
                elapsed_ms,
                ..
            } => {
                let item = self.upsert_thought(&thought_id);
                if let Some(thought) = item.thought_mut() {
                    thought.completed = true;
                    thought.elapsed_ms = Some(elapsed_ms);
                    thought.expanded = false;
                }
            }
            AppDisplayEvent::ToolStarted {
                tool_call_id,
                tool_name,
            } => {
                let tool = self.upsert_tool(&tool_call_id, &tool_name);
                tool.tool_name = tool_name;
                tool.completed = false;
            }
            AppDisplayEvent::ToolUpdated {
                tool_call_id,
                update,
            } => {
                let text = text_from_content_blocks(&update.content);
                self.upsert_tool(&tool_call_id, "").push_update(text);
            }
            AppDisplayEvent::ToolCompleted {
                tool_call_id,
                output,
            } => {
                let tool_output = tool_output_from_app(output);
                self.upsert_tool(&tool_call_id, "").complete(tool_output);
            }
            AppDisplayEvent::ApprovalRequested { approval } => {
                let approval = approval_card_from_app(approval);
                self.upsert_approval(approval);
            }
        }
    }

    pub fn toggle_thought_expanded(&mut self, thought_id: &str) -> bool {
        let Some(item) = self
            .transcript
            .iter_mut()
            .find(|item| item.message_id == thought_id)
        else {
            return false;
        };
        let Some(thought) = item.thought_mut() else {
            return false;
        };
        thought.expanded = !thought.expanded;
        true
    }

    pub fn toggle_tool_expanded(&mut self, tool_call_id: &str) -> bool {
        let Some(tool) = self
            .transcript
            .iter_mut()
            .find(|item| item.message_id == tool_call_id)
            .and_then(ChatTranscriptItem::tool_activity_mut)
        else {
            return false;
        };
        tool.expanded = !tool.expanded;
        true
    }

    fn upsert_thought(&mut self, thought_id: &str) -> &mut ChatTranscriptItem {
        if let Some(index) = self
            .transcript
            .iter()
            .position(|item| item.message_id == thought_id)
        {
            return &mut self.transcript[index];
        }
        self.transcript
            .push(ChatTranscriptItem::thought_item(thought_id));
        self.transcript
            .last_mut()
            .expect("thought item was just inserted")
    }

    fn upsert_tool(&mut self, tool_call_id: &str, tool_name: &str) -> &mut ChatToolActivity {
        if let Some(index) = self
            .transcript
            .iter()
            .position(|item| item.message_id == tool_call_id)
        {
            let item = &mut self.transcript[index];
            if item.tool_activity().is_none() {
                *item = ChatTranscriptItem::tool_item(tool_call_id, tool_name);
            }
            return item
                .tool_activity_mut()
                .expect("tool item was just converted");
        }
        self.transcript
            .push(ChatTranscriptItem::tool_item(tool_call_id, tool_name));
        self.transcript
            .last_mut()
            .and_then(ChatTranscriptItem::tool_activity_mut)
            .expect("tool item was just inserted")
    }

    fn upsert_approval(&mut self, approval: ChatApprovalCard) {
        if let Some(item) = self
            .transcript
            .iter_mut()
            .find(|item| item.message_id == approval.approval_id)
        {
            *item = ChatTranscriptItem::approval_item(approval);
            return;
        }
        self.transcript
            .push(ChatTranscriptItem::approval_item(approval));
    }

    fn set_current_run(&mut self, run: ChatRunState, session_status: AppInteractionSessionStatus) {
        self.current_run = Some(run);
        let Some(current_session_id) = self.current_session_id.as_deref() else {
            return;
        };
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|session| session.session_id == current_session_id)
        {
            session.status = session_status;
        }
        if let Some(descriptor) = self
            .descriptors
            .iter_mut()
            .find(|descriptor| descriptor.session_id == current_session_id)
        {
            descriptor.status = session_status;
        }
    }

    fn recover_current_transcript(&mut self, descriptors: &[AppInteractionSessionDescriptor]) {
        let Some(current_session_id) = self.current_session_id.as_deref() else {
            self.transcript.clear();
            self.current_run = None;
            return;
        };
        let Some(descriptor) = descriptors
            .iter()
            .find(|descriptor| descriptor.session_id == current_session_id)
        else {
            self.transcript.clear();
            self.current_run = None;
            return;
        };
        self.transcript = descriptor
            .state
            .messages
            .iter()
            .filter_map(|message| {
                let text = text_from_message(message)?;
                match message.role.as_str() {
                    "user" => Some(ChatTranscriptItem::user(message.id.clone(), text)),
                    "assistant" => Some(ChatTranscriptItem::assistant(
                        message.id.clone(),
                        text,
                        None,
                    )),
                    _ => None,
                }
            })
            .collect();
    }

    fn recover_current_run(&mut self, descriptors: &[AppInteractionSessionDescriptor]) {
        let Some(current_session_id) = self.current_session_id.as_deref() else {
            self.current_run = None;
            return;
        };
        let Some(descriptor) = descriptors
            .iter()
            .find(|descriptor| descriptor.session_id == current_session_id)
        else {
            self.current_run = None;
            return;
        };
        self.current_run = ChatRunState::from_session_status(descriptor.status);
    }

    fn merge_live_display_items(&mut self, previous: Vec<ChatTranscriptItem>) {
        let mut insert_at = self
            .transcript
            .iter()
            .rposition(|item| item.role() == ChatTranscriptRole::Assistant)
            .unwrap_or(self.transcript.len());
        for item in previous {
            if !matches!(
                item.role(),
                ChatTranscriptRole::Thought
                    | ChatTranscriptRole::Tool
                    | ChatTranscriptRole::Approval
            ) || self
                .transcript
                .iter()
                .any(|existing| existing.message_id == item.message_id)
            {
                continue;
            }
            self.transcript.insert(insert_at, item);
            insert_at += 1;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatSessionSummary {
    pub session_id: String,
    pub profile_id: String,
    pub status: AppInteractionSessionStatus,
    pub title: String,
}

impl From<&AppInteractionSessionDescriptor> for ChatSessionSummary {
    fn from(descriptor: &AppInteractionSessionDescriptor) -> Self {
        let title = descriptor
            .metadata
            .get("title")
            .and_then(|value| value.as_str())
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(&descriptor.session_id)
            .to_string();
        Self {
            session_id: descriptor.session_id.clone(),
            profile_id: descriptor.profile_id.clone(),
            status: descriptor.status,
            title,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatRunState {
    pub run_id: Option<String>,
    pub status: ChatRunStatus,
    pub error: Option<String>,
}

impl ChatRunState {
    fn from_session_status(status: AppInteractionSessionStatus) -> Option<Self> {
        let status = match status {
            AppInteractionSessionStatus::Idle => return None,
            AppInteractionSessionStatus::Running => ChatRunStatus::Running,
            AppInteractionSessionStatus::Completed => ChatRunStatus::Completed,
            AppInteractionSessionStatus::Aborted => ChatRunStatus::Aborted,
            AppInteractionSessionStatus::Failed => ChatRunStatus::Failed,
            AppInteractionSessionStatus::Paused => ChatRunStatus::Paused,
        };
        Some(Self {
            run_id: None,
            status,
            error: None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatRunStatus {
    Running,
    Completed,
    Aborted,
    Failed,
    Paused,
}

fn text_from_message(message: &AppMessage) -> Option<String> {
    let text = text_from_content_blocks(&message.content);
    if text.is_empty() { None } else { Some(text) }
}

fn text_from_content_blocks(blocks: &[AppContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            AppContentBlock::Text { text } => Some(text.as_str()),
            AppContentBlock::Other => None,
        })
        .collect::<String>()
}

fn tool_output_from_app(output: AppToolOutput) -> transcript::ChatToolOutput {
    let mut text = text_from_content_blocks(&output.content);
    for update in output.updates {
        text.push_str(&text_from_content_blocks(&update.content));
    }
    transcript::ChatToolOutput {
        text,
        is_error: output.is_error,
    }
}

fn approval_card_from_app(approval: AppToolApprovalRequest) -> ChatApprovalCard {
    ChatApprovalCard {
        approval_id: approval.approval_id,
        tool_call_id: approval.tool_call.id,
        tool_name: approval.tool_call.name,
        prompt: approval.request.prompt,
        reason: approval.request.reason,
        permissions: approval
            .permissions
            .into_iter()
            .map(|permission| {
                permission
                    .description
                    .filter(|description| !description.trim().is_empty())
                    .unwrap_or(permission.capability)
            })
            .collect(),
        status: ChatApprovalStatus::Pending,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;

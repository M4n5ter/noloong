use super::StreamingText;

const TOOL_OUTPUT_PREVIEW_CHARS: usize = 1_200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTranscriptItem {
    pub message_id: String,
    pub kind: ChatTranscriptItemKind,
}

impl ChatTranscriptItem {
    pub fn user(message_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            message_id: message_id.into(),
            kind: ChatTranscriptItemKind::User { text: text.into() },
        }
    }

    pub fn assistant(
        message_id: impl Into<String>,
        text: impl Into<String>,
        streaming: Option<StreamingText>,
    ) -> Self {
        Self {
            message_id: message_id.into(),
            kind: ChatTranscriptItemKind::Assistant {
                text: text.into(),
                streaming,
            },
        }
    }

    pub fn thought_item(message_id: impl Into<String>) -> Self {
        Self {
            message_id: message_id.into(),
            kind: ChatTranscriptItemKind::Thought(ChatThought::default()),
        }
    }

    pub fn tool_item(tool_call_id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        let tool_call_id = tool_call_id.into();
        Self {
            message_id: tool_call_id.clone(),
            kind: ChatTranscriptItemKind::Tool(ChatToolActivity::started(tool_call_id, tool_name)),
        }
    }

    pub fn approval_item(approval: ChatApprovalCard) -> Self {
        Self {
            message_id: approval.approval_id.clone(),
            kind: ChatTranscriptItemKind::Approval(approval),
        }
    }

    pub fn role(&self) -> ChatTranscriptRole {
        match self.kind {
            ChatTranscriptItemKind::User { .. } => ChatTranscriptRole::User,
            ChatTranscriptItemKind::Assistant { .. } => ChatTranscriptRole::Assistant,
            ChatTranscriptItemKind::Thought(_) => ChatTranscriptRole::Thought,
            ChatTranscriptItemKind::Tool(_) => ChatTranscriptRole::Tool,
            ChatTranscriptItemKind::Approval(_) => ChatTranscriptRole::Approval,
        }
    }

    pub fn text(&self) -> String {
        match &self.kind {
            ChatTranscriptItemKind::User { text }
            | ChatTranscriptItemKind::Assistant { text, .. } => text.clone(),
            ChatTranscriptItemKind::Thought(thought) => thought.display_text(),
            ChatTranscriptItemKind::Tool(tool) => tool.summary_text(),
            ChatTranscriptItemKind::Approval(approval) => approval.summary_text(),
        }
    }

    pub fn streaming(&self) -> Option<&StreamingText> {
        match &self.kind {
            ChatTranscriptItemKind::Assistant { streaming, .. } => streaming.as_ref(),
            ChatTranscriptItemKind::User { .. }
            | ChatTranscriptItemKind::Thought(_)
            | ChatTranscriptItemKind::Tool(_)
            | ChatTranscriptItemKind::Approval(_) => None,
        }
    }

    pub fn thought(&self) -> Option<&ChatThought> {
        match &self.kind {
            ChatTranscriptItemKind::Thought(thought) => Some(thought),
            ChatTranscriptItemKind::User { .. }
            | ChatTranscriptItemKind::Assistant { .. }
            | ChatTranscriptItemKind::Tool(_)
            | ChatTranscriptItemKind::Approval(_) => None,
        }
    }

    pub(crate) fn thought_mut(&mut self) -> Option<&mut ChatThought> {
        match &mut self.kind {
            ChatTranscriptItemKind::Thought(thought) => Some(thought),
            ChatTranscriptItemKind::User { .. }
            | ChatTranscriptItemKind::Assistant { .. }
            | ChatTranscriptItemKind::Tool(_)
            | ChatTranscriptItemKind::Approval(_) => None,
        }
    }

    pub fn tool_activity(&self) -> Option<&ChatToolActivity> {
        match &self.kind {
            ChatTranscriptItemKind::Tool(tool) => Some(tool),
            ChatTranscriptItemKind::User { .. }
            | ChatTranscriptItemKind::Assistant { .. }
            | ChatTranscriptItemKind::Thought(_)
            | ChatTranscriptItemKind::Approval(_) => None,
        }
    }

    pub fn tool(&self) -> Option<&ChatToolActivity> {
        self.tool_activity()
    }

    pub(crate) fn tool_activity_mut(&mut self) -> Option<&mut ChatToolActivity> {
        match &mut self.kind {
            ChatTranscriptItemKind::Tool(tool) => Some(tool),
            ChatTranscriptItemKind::User { .. }
            | ChatTranscriptItemKind::Assistant { .. }
            | ChatTranscriptItemKind::Thought(_)
            | ChatTranscriptItemKind::Approval(_) => None,
        }
    }

    pub fn approval_card(&self) -> Option<&ChatApprovalCard> {
        match &self.kind {
            ChatTranscriptItemKind::Approval(approval) => Some(approval),
            ChatTranscriptItemKind::User { .. }
            | ChatTranscriptItemKind::Assistant { .. }
            | ChatTranscriptItemKind::Thought(_)
            | ChatTranscriptItemKind::Tool(_) => None,
        }
    }

    pub fn approval(&self) -> Option<&ChatApprovalCard> {
        self.approval_card()
    }

    pub(crate) fn approval_mut(&mut self) -> Option<&mut ChatApprovalCard> {
        match &mut self.kind {
            ChatTranscriptItemKind::Approval(approval) => Some(approval),
            ChatTranscriptItemKind::User { .. }
            | ChatTranscriptItemKind::Assistant { .. }
            | ChatTranscriptItemKind::Thought(_)
            | ChatTranscriptItemKind::Tool(_) => None,
        }
    }

    pub(crate) fn push_assistant_delta(&mut self, text: String, now_ms: u64) {
        match &mut self.kind {
            ChatTranscriptItemKind::Assistant {
                text: current_text,
                streaming,
            } => {
                let streaming = streaming.get_or_insert_with(StreamingText::default);
                streaming.push_delta(text, now_ms);
                *current_text = streaming.text();
            }
            ChatTranscriptItemKind::User { .. }
            | ChatTranscriptItemKind::Thought(_)
            | ChatTranscriptItemKind::Tool(_)
            | ChatTranscriptItemKind::Approval(_) => {
                let mut streaming = StreamingText::default();
                streaming.push_delta(text, now_ms);
                self.kind = ChatTranscriptItemKind::Assistant {
                    text: streaming.text(),
                    streaming: Some(streaming),
                };
            }
        }
    }

    pub(crate) fn replace_with_assistant(
        &mut self,
        message_id: impl Into<String>,
        text: impl Into<String>,
    ) {
        self.message_id = message_id.into();
        self.kind = ChatTranscriptItemKind::Assistant {
            text: text.into(),
            streaming: None,
        };
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatTranscriptItemKind {
    User {
        text: String,
    },
    Assistant {
        text: String,
        streaming: Option<StreamingText>,
    },
    Thought(ChatThought),
    Tool(ChatToolActivity),
    Approval(ChatApprovalCard),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatTranscriptRole {
    User,
    Assistant,
    Thought,
    Tool,
    Approval,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatThought {
    pub summary: String,
    pub raw: String,
    pub completed: bool,
    pub elapsed_ms: Option<u64>,
    pub expanded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatToolActivity {
    pub tool_call_id: String,
    pub tool_name: String,
    pub updates: Vec<String>,
    pub output: Option<ChatToolOutput>,
    pub completed: bool,
    pub expanded: bool,
}

impl ChatToolActivity {
    pub fn started(tool_call_id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            updates: Vec::new(),
            output: None,
            completed: false,
            expanded: false,
        }
    }

    pub fn push_update(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !text.is_empty() {
            self.updates.push(text);
        }
    }

    pub fn complete(&mut self, output: ChatToolOutput) {
        self.output = Some(output);
        self.completed = true;
    }

    pub fn update_text(&self) -> String {
        self.updates.concat()
    }

    fn summary_text(&self) -> String {
        if self.completed {
            format!("{} completed", self.tool_name)
        } else {
            format!("{} running", self.tool_name)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatToolOutput {
    pub text: String,
    pub is_error: bool,
}

impl ChatToolOutput {
    pub fn is_long(&self) -> bool {
        self.text.chars().count() > TOOL_OUTPUT_PREVIEW_CHARS
    }

    pub fn preview_text(&self) -> String {
        if !self.is_long() {
            return self.text.clone();
        }
        let mut preview = self
            .text
            .chars()
            .take(TOOL_OUTPUT_PREVIEW_CHARS)
            .collect::<String>();
        preview.push('…');
        preview
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatApprovalCard {
    pub approval_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub prompt: Option<String>,
    pub reason: Option<String>,
    pub permissions: Vec<String>,
    pub status: ChatApprovalStatus,
}

impl ChatApprovalCard {
    pub fn resolve(&mut self, status: ChatApprovalStatus) {
        self.status = status;
    }

    fn summary_text(&self) -> String {
        match self.status {
            ChatApprovalStatus::Pending => format!("Approval required for {}", self.tool_name),
            ChatApprovalStatus::Allowed => format!("Approval approved for {}", self.tool_name),
            ChatApprovalStatus::Denied => format!("Approval rejected for {}", self.tool_name),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatApprovalStatus {
    Pending,
    Allowed,
    Denied,
}

impl ChatThought {
    pub(crate) fn push_delta(&mut self, kind: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        match kind {
            "summary" => self.summary.push_str(text),
            "raw" => self.raw.push_str(text),
            _ if self.summary.is_empty() => self.raw.push_str(text),
            _ => {}
        }
    }

    pub fn display_text(&self) -> String {
        if self.completed {
            return self.completed_text();
        }
        self.active_text()
    }

    pub fn active_text(&self) -> String {
        if !self.summary.is_empty() {
            self.summary.clone()
        } else if !self.raw.is_empty() {
            self.raw.clone()
        } else {
            "Thinking...".into()
        }
    }

    fn completed_text(&self) -> String {
        let elapsed_ms = self.elapsed_ms.unwrap_or_default();
        let seconds = ((elapsed_ms as f64) / 1000.0).round().max(1.0) as u64;
        if seconds == 1 {
            "Thought for 1 second".into()
        } else {
            format!("Thought for {seconds} seconds")
        }
    }
}

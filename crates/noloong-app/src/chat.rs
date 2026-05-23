use crate::interaction::{
    AppContentBlock, AppDisplayEvent, AppInteractionSessionDescriptor, AppInteractionSessionStatus,
    AppMessage,
};
use std::time::{SystemTime, UNIX_EPOCH};

mod composer;
mod streaming;

pub use composer::{ChatComposer, ChatComposerAction};
pub use streaming::StreamingText;

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
                    let streaming = item.streaming.get_or_insert_with(StreamingText::default);
                    streaming.push_delta(text, now_ms);
                    item.text = streaming.text();
                } else if !text.is_empty() {
                    let mut streaming = StreamingText::default();
                    streaming.push_delta(text, now_ms);
                    self.transcript.push(ChatTranscriptItem {
                        message_id: display_message_id,
                        role: ChatTranscriptRole::Assistant,
                        text: streaming.text(),
                        streaming: Some(streaming),
                        thought: None,
                    });
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
                    item.message_id = message.id;
                    item.role = ChatTranscriptRole::Assistant;
                    item.text = text;
                    item.streaming = None;
                    item.thought = None;
                } else {
                    self.transcript.push(ChatTranscriptItem {
                        message_id: message.id,
                        role: ChatTranscriptRole::Assistant,
                        text,
                        streaming: None,
                        thought: None,
                    });
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
                if let Some(thought) = item.thought.as_mut() {
                    thought.push_delta(&kind, &text);
                    item.text = thought.active_text();
                }
            }
            AppDisplayEvent::ThoughtCompleted {
                thought_id,
                elapsed_ms,
                ..
            } => {
                let item = self.upsert_thought(&thought_id);
                if let Some(thought) = item.thought.as_mut() {
                    thought.completed = true;
                    thought.elapsed_ms = Some(elapsed_ms);
                    thought.expanded = false;
                    item.text = thought.completed_text();
                }
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
        let Some(thought) = item.thought.as_mut() else {
            return false;
        };
        thought.expanded = !thought.expanded;
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
        self.transcript.push(ChatTranscriptItem {
            message_id: thought_id.into(),
            role: ChatTranscriptRole::Thought,
            text: ChatThought::default().active_text(),
            streaming: None,
            thought: Some(ChatThought::default()),
        });
        self.transcript
            .last_mut()
            .expect("thought item was just inserted")
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
                let role = match message.role.as_str() {
                    "user" => ChatTranscriptRole::User,
                    "assistant" => ChatTranscriptRole::Assistant,
                    _ => return None,
                };
                let text = text_from_message(message)?;
                Some(ChatTranscriptItem {
                    message_id: message.id.clone(),
                    role,
                    text,
                    streaming: None,
                    thought: None,
                })
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
pub struct ChatTranscriptItem {
    pub message_id: String,
    pub role: ChatTranscriptRole,
    pub text: String,
    pub streaming: Option<StreamingText>,
    pub thought: Option<ChatThought>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatTranscriptRole {
    User,
    Assistant,
    Thought,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatThought {
    pub summary: String,
    pub raw: String,
    pub completed: bool,
    pub elapsed_ms: Option<u64>,
    pub expanded: bool,
}

impl ChatThought {
    fn push_delta(&mut self, kind: &str, text: &str) {
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

    fn active_text(&self) -> String {
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

fn text_from_message(message: &AppMessage) -> Option<String> {
    let text = message
        .content
        .iter()
        .filter_map(|block| match block {
            AppContentBlock::Text { text } => Some(text.as_str()),
            AppContentBlock::Other => None,
        })
        .collect::<String>();
    if text.is_empty() { None } else { Some(text) }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        ChatComposer, ChatComposerAction, ChatRunStatus, ChatSessionStore, ChatTranscriptRole,
        StreamingText,
    };
    use crate::interaction::{
        AppContentBlock, AppDisplayEvent, AppInteractionSessionDescriptor,
        AppInteractionSessionState, AppInteractionSessionStatus, AppMessage,
    };

    #[test]
    fn display_delta_streams_then_final_replaces_the_same_assistant_bubble() {
        let mut store = ChatSessionStore::default();
        store.refresh(vec![session_descriptor(
            "session-1",
            AppInteractionSessionStatus::Running,
            Vec::new(),
        )]);

        store.apply_display_event(AppDisplayEvent::AssistantMessageDelta {
            run_id: "run-1".into(),
            display_message_id: "run-1:assistant".into(),
            text: "hel".into(),
        });
        store.apply_display_event(AppDisplayEvent::AssistantMessageDelta {
            run_id: "run-1".into(),
            display_message_id: "run-1:assistant".into(),
            text: "lo".into(),
        });

        assert_eq!(store.transcript().len(), 1);
        assert_eq!(store.transcript()[0].role, ChatTranscriptRole::Assistant);
        assert_eq!(store.transcript()[0].text, "hello");
        assert!(store.transcript()[0].streaming.is_some());

        store.apply_display_event(AppDisplayEvent::AssistantMessageFinal {
            run_id: "run-1".into(),
            display_message_id: "run-1:assistant".into(),
            message: message("assistant-1", "assistant", "hello!"),
            truncated: false,
        });

        assert_eq!(store.transcript().len(), 1);
        assert_eq!(store.transcript()[0].message_id, "assistant-1");
        assert_eq!(store.transcript()[0].text, "hello!");
        assert_eq!(store.transcript()[0].streaming, None);
    }

    #[test]
    fn streaming_segments_ramp_from_dim_to_stable_opacity() {
        let mut stream = StreamingText::default();

        stream.push_delta("hel", 1_000);
        stream.push_delta("lo", 1_080);

        let fresh = stream.visible_segments(1_080);
        assert_eq!(fresh[0].text, "hel");
        assert_eq!(fresh[0].opacity, 0.7);
        assert_eq!(fresh[1].text, "lo");
        assert_eq!(fresh[1].opacity, 0.35);

        let stable = stream.visible_segments(1_260);
        assert_eq!(stable[0].opacity, 1.0);
        assert_eq!(stable[1].opacity, 1.0);
        assert_eq!(stream.text(), "hello");
    }

    #[test]
    fn composer_enter_submits_non_empty_text_and_shift_enter_adds_newline() {
        let mut composer = ChatComposer::default();
        assert!(!composer.can_send());
        assert_eq!(composer.press_enter(false), ChatComposerAction::None);

        composer.set_text("hello".into());
        assert!(composer.can_send());
        assert_eq!(
            composer.press_enter(true),
            ChatComposerAction::InsertNewline
        );
        assert_eq!(
            composer.press_enter(false),
            ChatComposerAction::Submit("hello".into())
        );
        assert!(!composer.can_send());
    }

    #[test]
    fn thought_summary_takes_priority_over_raw_and_completion_collapses() {
        let mut store = ChatSessionStore::default();
        store.refresh(vec![session_descriptor(
            "session-1",
            AppInteractionSessionStatus::Running,
            Vec::new(),
        )]);

        store.apply_display_event(AppDisplayEvent::ThoughtStarted {
            run_id: "run-1".into(),
            thought_id: "run-1:thought".into(),
        });
        store.apply_display_event(AppDisplayEvent::ThoughtDelta {
            run_id: "run-1".into(),
            thought_id: "run-1:thought".into(),
            kind: "raw".into(),
            text: "raw detail".into(),
        });
        store.apply_display_event(AppDisplayEvent::ThoughtDelta {
            run_id: "run-1".into(),
            thought_id: "run-1:thought".into(),
            kind: "summary".into(),
            text: "summary".into(),
        });

        assert_eq!(store.transcript().len(), 1);
        let item = &store.transcript()[0];
        assert_eq!(item.role, ChatTranscriptRole::Thought);
        assert_eq!(item.text, "summary");
        let thought = item.thought.as_ref().expect("thought state");
        assert_eq!(thought.summary, "summary");
        assert_eq!(thought.raw, "raw detail");
        assert!(!thought.completed);

        store.apply_display_event(AppDisplayEvent::ThoughtCompleted {
            run_id: "run-1".into(),
            thought_id: "run-1:thought".into(),
            elapsed_ms: 2_000,
        });

        let item = &store.transcript()[0];
        assert_eq!(item.text, "Thought for 2 seconds");
        let thought = item.thought.as_ref().expect("thought state");
        assert!(thought.completed);
        assert_eq!(thought.elapsed_ms, Some(2_000));
        assert!(!thought.expanded);

        assert!(store.toggle_thought_expanded("run-1:thought"));
        assert!(store.transcript()[0].thought.as_ref().unwrap().expanded);
    }

    #[test]
    fn run_lifecycle_updates_current_status_and_composer_availability() {
        let mut store = ChatSessionStore::default();
        store.refresh(vec![session_descriptor(
            "session-1",
            AppInteractionSessionStatus::Idle,
            Vec::new(),
        )]);

        store.apply_display_event(AppDisplayEvent::RunStarted {
            run_id: "run-1".into(),
        });
        assert_eq!(
            store.current_run().map(|run| run.status),
            Some(ChatRunStatus::Running)
        );
        assert_eq!(
            store.sessions()[0].status,
            AppInteractionSessionStatus::Running
        );
        assert!(!store.can_send_current_message());

        store.apply_display_event(AppDisplayEvent::RunPaused {
            run_id: "run-1".into(),
            reason: serde_json::json!({"type": "approval_required"}),
        });
        assert_eq!(
            store.current_run().map(|run| run.status),
            Some(ChatRunStatus::Paused)
        );
        assert_eq!(
            store.sessions()[0].status,
            AppInteractionSessionStatus::Paused
        );
        assert!(!store.can_send_current_message());

        store.apply_display_event(AppDisplayEvent::RunAborted {
            run_id: "run-1".into(),
        });
        assert_eq!(
            store.current_run().map(|run| run.status),
            Some(ChatRunStatus::Aborted)
        );
        assert_eq!(
            store.sessions()[0].status,
            AppInteractionSessionStatus::Aborted
        );
        assert!(store.can_send_current_message());
    }

    #[test]
    fn run_failure_keeps_error_visible_until_next_run_starts() {
        let mut store = ChatSessionStore::default();
        store.refresh(vec![session_descriptor(
            "session-1",
            AppInteractionSessionStatus::Running,
            Vec::new(),
        )]);

        store.apply_display_event(AppDisplayEvent::RunFailed {
            run_id: "run-1".into(),
            error: "provider 400".into(),
        });

        assert_eq!(
            store.current_run().map(|run| run.status),
            Some(ChatRunStatus::Failed)
        );
        assert_eq!(
            store.current_run().and_then(|run| run.error.as_deref()),
            Some("provider 400")
        );
        assert!(store.can_send_current_message());

        store.apply_display_event(AppDisplayEvent::RunStarted {
            run_id: "run-2".into(),
        });
        assert_eq!(
            store.current_run().map(|run| run.status),
            Some(ChatRunStatus::Running)
        );
        assert_eq!(
            store.current_run().and_then(|run| run.error.as_deref()),
            None
        );
    }

    #[test]
    fn connection_error_is_visible_until_next_run_starts() {
        let mut store = ChatSessionStore::default();
        store.refresh(vec![session_descriptor(
            "session-1",
            AppInteractionSessionStatus::Running,
            Vec::new(),
        )]);

        store.set_connection_error("websocket closed".into());

        assert_eq!(store.connection_error(), Some("websocket closed"));

        store.apply_display_event(AppDisplayEvent::RunStarted {
            run_id: "run-2".into(),
        });

        assert_eq!(store.connection_error(), None);
    }

    fn session_descriptor(
        session_id: &str,
        status: AppInteractionSessionStatus,
        messages: Vec<AppMessage>,
    ) -> AppInteractionSessionDescriptor {
        AppInteractionSessionDescriptor {
            session_id: session_id.into(),
            profile_id: "default".into(),
            parent_session_id: None,
            role: None,
            status,
            state: AppInteractionSessionState { messages },
            metadata: Default::default(),
        }
    }

    fn message(id: &str, role: &str, text: &str) -> AppMessage {
        AppMessage {
            id: id.into(),
            role: role.into(),
            content: vec![AppContentBlock::Text { text: text.into() }],
            metadata: Default::default(),
        }
    }
}

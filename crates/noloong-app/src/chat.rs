use crate::interaction::{
    AppContentBlock, AppInteractionSessionDescriptor, AppInteractionSessionStatus,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatSessionStore {
    descriptors: Vec<AppInteractionSessionDescriptor>,
    sessions: Vec<ChatSessionSummary>,
    current_session_id: Option<String>,
    transcript: Vec<ChatTranscriptItem>,
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

    fn recover_current_transcript(&mut self, descriptors: &[AppInteractionSessionDescriptor]) {
        let Some(current_session_id) = self.current_session_id.as_deref() else {
            self.transcript.clear();
            return;
        };
        let Some(descriptor) = descriptors
            .iter()
            .find(|descriptor| descriptor.session_id == current_session_id)
        else {
            self.transcript.clear();
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
                let text = message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        AppContentBlock::Text { text } => Some(text.as_str()),
                        AppContentBlock::Other => None,
                    })
                    .collect::<String>();
                if text.is_empty() {
                    return None;
                }
                Some(ChatTranscriptItem {
                    message_id: message.id.clone(),
                    role,
                    text,
                })
            })
            .collect();
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatTranscriptRole {
    User,
    Assistant,
}

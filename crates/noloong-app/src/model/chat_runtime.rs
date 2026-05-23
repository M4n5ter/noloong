#[cfg(test)]
use super::AppError;
use super::AppViewModel;
use crate::chat::{ChatSessionSummary, ChatTranscriptItem};
#[cfg(test)]
use crate::interaction::{
    AppInteractionClient, AppPromptInput, AppPromptRequest, AppSessionCreateRequest,
};
use crate::interaction::{
    AppInteractionDisplayNotification, AppInteractionSessionDescriptor, AppInteractionStatus,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatEmptyState {
    MissingConfig,
    Connecting,
    ConnectionFailed(String),
    NoSession,
}

impl AppViewModel {
    pub fn chat_empty_state(&self) -> ChatEmptyState {
        if self.chat.current_session_id().is_some() {
            return ChatEmptyState::NoSession;
        }
        match &self.interaction_status {
            AppInteractionStatus::Ready { .. } => ChatEmptyState::NoSession,
            AppInteractionStatus::Pending => ChatEmptyState::Connecting,
            AppInteractionStatus::Failed(error) => ChatEmptyState::ConnectionFailed(error.clone()),
            AppInteractionStatus::Unavailable => ChatEmptyState::MissingConfig,
        }
    }

    #[cfg(test)]
    pub async fn refresh_chat_sessions(
        &mut self,
        client: &impl AppInteractionClient,
    ) -> Result<(), AppError> {
        let sessions = client
            .list_sessions()
            .await
            .map_err(|error| AppError::Interaction(error.to_string()))?;
        self.chat.refresh(sessions);
        Ok(())
    }

    pub fn apply_chat_session_descriptors(
        &mut self,
        sessions: Vec<AppInteractionSessionDescriptor>,
    ) {
        self.chat.refresh(sessions);
    }

    pub fn apply_chat_session_descriptor(&mut self, session: AppInteractionSessionDescriptor) {
        self.chat.upsert_and_select(session);
    }

    pub fn apply_display_notification(&mut self, notification: AppInteractionDisplayNotification) {
        if self.current_chat_session_id() == Some(notification.session_id.as_str()) {
            self.chat.apply_display_event(notification.event);
        }
    }

    #[cfg(test)]
    pub async fn create_chat_session(
        &mut self,
        client: &impl AppInteractionClient,
    ) -> Result<(), AppError> {
        let descriptor = client
            .create_session(AppSessionCreateRequest {
                session_id: None,
                profile_id: self.selected_profile_id.clone(),
                metadata: Default::default(),
            })
            .await
            .map_err(|error| AppError::Interaction(error.to_string()))?;
        self.chat.upsert_and_select(descriptor);
        Ok(())
    }

    #[cfg(test)]
    pub async fn submit_chat_message(
        &mut self,
        client: &impl AppInteractionClient,
        text: String,
    ) -> Result<(), AppError> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }
        let session_id = match self.current_chat_session_id().map(str::to_string) {
            Some(session_id) => session_id,
            None => {
                let descriptor = client
                    .create_session(AppSessionCreateRequest {
                        session_id: None,
                        profile_id: self.selected_profile_id.clone(),
                        metadata: Default::default(),
                    })
                    .await
                    .map_err(|error| AppError::Interaction(error.to_string()))?;
                let session_id = descriptor.session_id.clone();
                self.chat.upsert_and_select(descriptor);
                session_id
            }
        };
        let descriptor = client
            .prompt(AppPromptRequest {
                session_id,
                input: AppPromptInput::Text { text },
            })
            .await
            .map_err(|error| AppError::Interaction(error.to_string()))?;
        self.chat.upsert_and_select(descriptor);
        Ok(())
    }

    #[cfg(test)]
    pub async fn refresh_current_chat_session(
        &mut self,
        client: &impl AppInteractionClient,
    ) -> Result<(), AppError> {
        let Some(session_id) = self.current_chat_session_id().map(str::to_string) else {
            return Ok(());
        };
        let descriptor = client
            .get_session(&session_id)
            .await
            .map_err(|error| AppError::Interaction(error.to_string()))?;
        self.chat.upsert_and_select(descriptor);
        Ok(())
    }

    pub fn chat_sessions(&self) -> &[ChatSessionSummary] {
        self.chat.sessions()
    }

    pub fn current_chat_session_id(&self) -> Option<&str> {
        self.chat.current_session_id()
    }

    pub fn select_chat_session(&mut self, session_id: &str) -> bool {
        self.chat.select_session(session_id)
    }

    pub fn chat_transcript(&self) -> &[ChatTranscriptItem] {
        self.chat.transcript()
    }
}

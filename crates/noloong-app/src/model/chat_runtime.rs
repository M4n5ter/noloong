#[cfg(test)]
use super::AppError;
use super::AppViewModel;
use crate::chat::{
    ChatApprovalStatus, ChatRunState, ChatRunStatus, ChatSessionSummary, ChatTranscriptItem,
    session_metadata_for_prompt_in_workdir,
};
#[cfg(test)]
use crate::chat::{SESSION_TITLE_METADATA_KEY, SESSION_WORKDIR_METADATA_KEY};
#[cfg(test)]
use crate::interaction::{
    AppApprovalResolveRequest, AppInteractionClient, AppPromptInput, AppPromptRequest,
    AppSessionCreateRequest, AppSessionMetadataUpdateRequest, AppSessionRequest,
    AppToolPermissionDecision,
};
use crate::interaction::{
    AppInteractionDisplayNotification, AppInteractionSessionDescriptor, AppInteractionStatus,
    AppToolPermissionOutcome,
};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

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
        let input = AppPromptInput::Text { text };
        let session_id = match self.current_chat_session_id().map(str::to_string) {
            Some(session_id) => session_id,
            None => {
                let descriptor = client
                    .create_session(AppSessionCreateRequest {
                        session_id: None,
                        profile_id: self.selected_profile_id.clone(),
                        metadata: self.chat_session_metadata_for_prompt(&input),
                    })
                    .await
                    .map_err(|error| AppError::Interaction(error.to_string()))?;
                let session_id = descriptor.session_id.clone();
                self.chat.upsert_and_select(descriptor);
                session_id
            }
        };
        let descriptor = client
            .prompt(AppPromptRequest { session_id, input })
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

    #[cfg(test)]
    pub async fn rename_current_chat_session(
        &mut self,
        client: &impl AppInteractionClient,
        title: String,
    ) -> Result<(), AppError> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Ok(());
        }
        let Some(session_id) = self.current_chat_session_id().map(str::to_string) else {
            return Ok(());
        };
        let descriptor = client
            .update_session_metadata(AppSessionMetadataUpdateRequest {
                session_id,
                metadata: [(SESSION_TITLE_METADATA_KEY.into(), serde_json::json!(title))]
                    .into_iter()
                    .collect(),
            })
            .await
            .map_err(|error| AppError::Interaction(error.to_string()))?;
        self.chat.upsert_and_select(descriptor);
        Ok(())
    }

    #[cfg(test)]
    pub async fn update_current_chat_workdir(
        &mut self,
        client: &impl AppInteractionClient,
        workdir: PathBuf,
    ) -> Result<(), AppError> {
        let Some(session_id) = self.current_chat_session_id().map(str::to_string) else {
            self.set_chat_workdir(workdir);
            return Ok(());
        };
        let descriptor = client
            .update_session_metadata(AppSessionMetadataUpdateRequest {
                session_id,
                metadata: [(
                    SESSION_WORKDIR_METADATA_KEY.into(),
                    serde_json::json!(workdir.display().to_string()),
                )]
                .into_iter()
                .collect(),
            })
            .await
            .map_err(|error| AppError::Interaction(error.to_string()))?;
        self.set_chat_workdir(workdir);
        self.chat.upsert_and_select(descriptor);
        Ok(())
    }

    #[cfg(test)]
    pub async fn abort_current_chat_run(
        &mut self,
        client: &impl AppInteractionClient,
    ) -> Result<(), AppError> {
        let Some(session_id) = self.current_chat_session_id().map(str::to_string) else {
            return Ok(());
        };
        let descriptor = client
            .abort(AppSessionRequest { session_id })
            .await
            .map_err(|error| AppError::Interaction(error.to_string()))?;
        self.chat.upsert_and_select(descriptor);
        Ok(())
    }

    #[cfg(test)]
    pub async fn resolve_chat_approval(
        &mut self,
        client: &impl AppInteractionClient,
        approval_id: String,
        outcome: AppToolPermissionOutcome,
    ) -> Result<(), AppError> {
        let Some(session_id) = self.current_chat_session_id().map(str::to_string) else {
            return Ok(());
        };
        let descriptor = client
            .resolve_approval(AppApprovalResolveRequest {
                session_id,
                approval_id: approval_id.clone(),
                decision: AppToolPermissionDecision::from_outcome(outcome),
            })
            .await
            .map_err(|error| AppError::Interaction(error.to_string()))?;
        self.apply_chat_approval_resolution(&approval_id, outcome, descriptor);
        Ok(())
    }

    pub fn apply_chat_approval_resolution(
        &mut self,
        approval_id: &str,
        outcome: AppToolPermissionOutcome,
        descriptor: AppInteractionSessionDescriptor,
    ) {
        let status = match outcome {
            AppToolPermissionOutcome::Allow => ChatApprovalStatus::Allowed,
            AppToolPermissionOutcome::Deny => ChatApprovalStatus::Denied,
        };
        self.chat.resolve_approval(approval_id, status);
        self.chat
            .update_session_descriptor_preserving_transcript(descriptor);
    }

    pub fn current_chat_run(&self) -> Option<&ChatRunState> {
        self.chat.current_run()
    }

    pub fn chat_connection_error(&self) -> Option<&str> {
        self.chat.connection_error()
    }

    pub fn record_chat_connection_error(&mut self, error: String) {
        self.chat.set_connection_error(error.clone());
        self.interaction_status = AppInteractionStatus::Failed(error);
    }

    pub fn can_send_chat_message(&self) -> bool {
        self.chat.can_send_current_message()
    }

    pub fn can_abort_current_chat_run(&self) -> bool {
        matches!(
            self.chat.current_run().map(|run| run.status),
            Some(ChatRunStatus::Running | ChatRunStatus::Paused)
        )
    }

    pub fn chat_sessions(&self) -> &[ChatSessionSummary] {
        self.chat.sessions()
    }

    pub fn chat_workdir(&self) -> &Path {
        self.chat
            .current_session()
            .map(|session| Path::new(session.workdir.as_str()))
            .unwrap_or(self.chat_workdir.as_path())
    }

    pub fn set_chat_workdir(&mut self, workdir: PathBuf) {
        self.chat_workdir = workdir;
    }

    pub fn chat_session_metadata_for_prompt(
        &self,
        input: &crate::interaction::AppPromptInput,
    ) -> Map<String, Value> {
        session_metadata_for_prompt_in_workdir(input, self.chat_workdir.as_path())
    }

    pub fn current_chat_context(&self) -> Option<super::ChatContextSummary> {
        let session = self.chat.current_session()?;
        let profile = self
            .config
            .profiles
            .iter()
            .find(|profile| profile.profile_id == session.profile_id);
        Some(super::ChatContextSummary {
            title: session.title.clone(),
            profile_id: session.profile_id.clone(),
            profile_name: profile
                .map(|profile| profile.display_name.clone())
                .unwrap_or_else(|| session.profile_id.clone()),
            model: profile
                .map(|profile| profile.provider.model().to_string())
                .unwrap_or_default(),
            workdir: session.workdir.clone(),
        })
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

    pub fn append_local_chat_user_message(
        &mut self,
        message_id: String,
        input: &crate::interaction::AppPromptInput,
    ) -> bool {
        self.chat.append_local_user_message(message_id, input)
    }

    pub fn toggle_thought_expanded(&mut self, thought_id: &str) -> bool {
        self.chat.toggle_thought_expanded(thought_id)
    }

    pub fn toggle_tool_expanded(&mut self, tool_call_id: &str) -> bool {
        self.chat.toggle_tool_expanded(tool_call_id)
    }
}

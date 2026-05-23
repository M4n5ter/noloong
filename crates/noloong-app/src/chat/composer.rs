use crate::interaction::{
    AppContentBlock, AppMediaBlock, AppMediaKind, AppMediaSource, AppMessage, AppPromptInput,
};
use serde_json::Map;
use std::{fmt, fs, path::PathBuf};
use url::Url;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatComposer {
    text: String,
    attachments: Vec<ChatAttachmentDraft>,
}

impl ChatComposer {
    pub fn set_text(&mut self, text: String) {
        self.text = text;
    }

    pub fn attachments(&self) -> &[ChatAttachmentDraft] {
        &self.attachments
    }

    pub fn set_attachments(&mut self, attachments: Vec<ChatAttachmentDraft>) {
        self.attachments = attachments;
    }

    pub fn into_attachments(self) -> Vec<ChatAttachmentDraft> {
        self.attachments
    }

    pub fn add_attachment_path(
        &mut self,
        path: impl Into<PathBuf>,
    ) -> Result<(), ChatAttachmentError> {
        let attachment = ChatAttachmentDraft::from_path(path.into())?;
        if !self
            .attachments
            .iter()
            .any(|existing| existing.id == attachment.id)
        {
            self.attachments.push(attachment);
        }
        Ok(())
    }

    pub fn remove_attachment(&mut self, attachment_id: &str) -> bool {
        let previous_len = self.attachments.len();
        self.attachments
            .retain(|attachment| attachment.id != attachment_id);
        self.attachments.len() != previous_len
    }

    pub fn can_send(&self) -> bool {
        !self.text.trim().is_empty() || !self.attachments.is_empty()
    }

    pub fn press_enter(&mut self, shift: bool) -> ChatComposerAction {
        if shift {
            self.text.push('\n');
            return ChatComposerAction::InsertNewline;
        }
        let text = self.text.trim().to_string();
        if text.is_empty() && self.attachments.is_empty() {
            return ChatComposerAction::None;
        }
        let attachments = std::mem::take(&mut self.attachments);
        self.text.clear();
        ChatComposerAction::Submit(ChatComposerSubmission { text, attachments })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatComposerAction {
    Submit(ChatComposerSubmission),
    InsertNewline,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatComposerSubmission {
    pub text: String,
    pub attachments: Vec<ChatAttachmentDraft>,
}

impl ChatComposerSubmission {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            attachments: Vec::new(),
        }
    }

    pub fn into_prompt_input(self, message_id: impl Into<String>) -> AppPromptInput {
        if self.attachments.is_empty() {
            return AppPromptInput::Text { text: self.text };
        }
        let mut content = Vec::new();
        if !self.text.trim().is_empty() {
            content.push(AppContentBlock::Text { text: self.text });
        }
        content.extend(
            self.attachments
                .into_iter()
                .map(ChatAttachmentDraft::into_content_block),
        );
        AppPromptInput::Message {
            message: AppMessage {
                id: message_id.into(),
                role: "user".into(),
                content,
                metadata: Map::new(),
            },
        }
    }
}

impl From<&str> for ChatComposerSubmission {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl From<String> for ChatComposerSubmission {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

impl PartialEq<&str> for ChatComposerSubmission {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other && self.attachments.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatAttachmentDraft {
    pub id: String,
    pub path: PathBuf,
    pub file_name: String,
    pub kind: AppMediaKind,
    pub mime_type: Option<String>,
}

impl ChatAttachmentDraft {
    pub fn from_path(path: PathBuf) -> Result<Self, ChatAttachmentError> {
        let metadata = fs::metadata(&path)
            .map_err(|error| ChatAttachmentError::Unreadable(path.clone(), error.to_string()))?;
        if !metadata.is_file() {
            return Err(ChatAttachmentError::NotAFile(path));
        }
        fs::File::open(&path)
            .map_err(|error| ChatAttachmentError::Unreadable(path.clone(), error.to_string()))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| ChatAttachmentError::MissingFileName(path.clone()))?
            .to_string();
        let mime_type = mime_guess::from_path(&path).first_raw().map(str::to_string);
        let kind = AppMediaKind::from_mime_type(mime_type.as_deref());
        let uri = file_uri(&path)?;
        Ok(Self {
            id: uri,
            path,
            file_name,
            kind,
            mime_type,
        })
    }

    pub fn into_content_block(self) -> AppContentBlock {
        AppContentBlock::Media {
            media: AppMediaBlock {
                kind: self.kind,
                source: AppMediaSource::Uri { uri: self.id },
                mime_type: self.mime_type,
                name: Some(self.file_name),
                metadata: Map::new(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatAttachmentError {
    NotAFile(PathBuf),
    MissingFileName(PathBuf),
    Unreadable(PathBuf, String),
    InvalidFileUri(PathBuf),
}

impl fmt::Display for ChatAttachmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAFile(path) => write!(formatter, "{} is not a file", path.display()),
            Self::MissingFileName(path) => write!(formatter, "{} has no file name", path.display()),
            Self::Unreadable(path, error) => {
                write!(formatter, "{} is unreadable: {error}", path.display())
            }
            Self::InvalidFileUri(path) => {
                write!(
                    formatter,
                    "{} cannot be represented as a file URI",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ChatAttachmentError {}

fn file_uri(path: &PathBuf) -> Result<String, ChatAttachmentError> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| ChatAttachmentError::InvalidFileUri(path.clone()))
}

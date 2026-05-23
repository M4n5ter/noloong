use crate::interaction::{AppContentBlock, AppPromptInput};
use serde_json::{Map, Value, json};

pub const SESSION_TITLE_METADATA_KEY: &str = "title";
pub const SESSION_WORKDIR_METADATA_KEY: &str = "workdir";
const MAX_GENERATED_TITLE_CHARS: usize = 48;

pub fn session_metadata_for_prompt(input: &AppPromptInput) -> Map<String, Value> {
    let mut metadata = Map::new();
    if let Some(title) = generated_title_from_prompt(input) {
        metadata.insert(SESSION_TITLE_METADATA_KEY.into(), json!(title));
    }
    if let Ok(workdir) = std::env::current_dir() {
        metadata.insert(
            SESSION_WORKDIR_METADATA_KEY.into(),
            json!(workdir.display().to_string()),
        );
    }
    metadata
}

fn generated_title_from_prompt(input: &AppPromptInput) -> Option<String> {
    let title = match input {
        AppPromptInput::Text { text } => text.as_str(),
        AppPromptInput::Message { message } => message
            .content
            .iter()
            .find_map(title_text_from_content_block)
            .unwrap_or_default(),
    };
    compact_title(title)
}

fn title_text_from_content_block(block: &AppContentBlock) -> Option<&str> {
    match block {
        AppContentBlock::Text { text } => Some(text.as_str()),
        AppContentBlock::Media { media } => media.name.as_deref(),
        AppContentBlock::Other => None,
    }
}

fn compact_title(text: &str) -> Option<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }
    let mut chars = normalized.chars();
    let title = chars
        .by_ref()
        .take(MAX_GENERATED_TITLE_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        Some(format!("{title}..."))
    } else {
        Some(title)
    }
}

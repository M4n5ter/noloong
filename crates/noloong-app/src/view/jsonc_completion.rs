use anyhow::Result;
use gpui::{AppContext as _, Context, Task, Window};
use gpui_component::input::{CompletionProvider, InputState, Rope, RopeExt as _};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    Position, TextEdit,
};
use noloong_config::schema::{ProfileConfigSchemaCompletionKind, ProfileConfigSchemaIndex};

pub(super) struct ProfileJsoncCompletionProvider {
    schema_index: ProfileConfigSchemaIndex,
}

impl ProfileJsoncCompletionProvider {
    pub(super) fn new(schema_index: ProfileConfigSchemaIndex) -> Self {
        Self { schema_index }
    }
}

impl CompletionProvider for ProfileJsoncCompletionProvider {
    fn completions(
        &self,
        rope: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        cx: &mut Context<InputState>,
    ) -> Task<Result<CompletionResponse>> {
        let text = rope.slice(0..offset).to_string();
        let end = rope.offset_to_position(offset);
        let schema_index = self.schema_index.clone();

        cx.background_spawn(async move {
            let completion_set = schema_index.completions_for_text(&text, text.len());
            let start = offset_to_position(&text, completion_set.replace_start.min(text.len()));
            let items = completion_set
                .completions
                .into_iter()
                .map(|completion| CompletionItem {
                    label: completion.label,
                    kind: Some(match completion.kind {
                        ProfileConfigSchemaCompletionKind::Property => CompletionItemKind::PROPERTY,
                        ProfileConfigSchemaCompletionKind::Value => CompletionItemKind::VALUE,
                        ProfileConfigSchemaCompletionKind::Snippet => CompletionItemKind::SNIPPET,
                    }),
                    detail: completion.detail,
                    documentation: completion
                        .documentation
                        .map(lsp_types::Documentation::String),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range: lsp_types::Range { start, end },
                        new_text: completion.insert_text,
                    })),
                    ..Default::default()
                })
                .collect::<Vec<_>>();
            Ok(CompletionResponse::Array(items))
        })
    }

    fn is_completion_trigger(&self, _: usize, new_text: &str, _: &mut Context<InputState>) -> bool {
        new_text.is_empty()
            || new_text.chars().any(|ch| {
                matches!(ch, '"' | ':' | ',' | '{' | '[') || ch.is_ascii_alphanumeric() || ch == '_'
            })
    }
}

fn offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut line_start = 0;
    for (index, ch) in text.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = index + ch.len_utf8();
        }
    }
    Position::new(
        line,
        text[line_start..offset.min(text.len())].chars().count() as u32,
    )
}

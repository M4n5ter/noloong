use noloong_agent_core::{AgentMessage, ContentBlock};
use std::borrow::Cow;

pub fn render_agent_message_text(message: &AgentMessage) -> String {
    message
        .content
        .iter()
        .filter_map(render_content_block_text)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_markdown_v2(text: &str) -> String {
    let text = if text.contains('|') {
        Cow::Owned(rewrite_pipe_tables(text))
    } else {
        Cow::Borrowed(text)
    };
    text.split("```")
        .enumerate()
        .map(|(index, segment)| {
            if index % 2 == 1 {
                format!("```{segment}```")
            } else {
                escape_markdown_v2(segment)
            }
        })
        .collect::<String>()
}

pub fn escape_markdown_v2(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(
            ch,
            '_' | '*'
                | '['
                | ']'
                | '('
                | ')'
                | '~'
                | '`'
                | '>'
                | '#'
                | '+'
                | '-'
                | '='
                | '|'
                | '{'
                | '}'
                | '.'
                | '!'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

pub fn rewrite_pipe_tables(text: &str) -> String {
    let mut output = Vec::new();
    let lines = text.lines().collect::<Vec<_>>();
    let mut index = 0;
    while index < lines.len() {
        if index + 1 < lines.len() && is_pipe_row(lines[index]) && is_separator(lines[index + 1]) {
            let headers = split_pipe_row(lines[index]);
            index += 2;
            while index < lines.len() && is_pipe_row(lines[index]) {
                let cells = split_pipe_row(lines[index]);
                let pairs = headers
                    .iter()
                    .zip(cells.iter())
                    .map(|(header, cell)| format!("{header}: {cell}"))
                    .collect::<Vec<_>>();
                output.push(format!("- {}", pairs.join(", ")));
                index += 1;
            }
        } else {
            output.push(lines[index].into());
            index += 1;
        }
    }
    output.join("\n")
}

pub(crate) fn render_content_block_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text { text } => Some(text.clone()),
        ContentBlock::Json { value } => Some(value.to_string()),
        ContentBlock::Thinking { .. } => None,
        ContentBlock::Media { .. } => Some("[media]".into()),
        ContentBlock::ToolCall { tool_call } => Some(format!("Tool call: {}", tool_call.name)),
        ContentBlock::ToolResult {
            tool_name,
            content,
            is_error,
            ..
        } => {
            let text = content
                .iter()
                .filter_map(render_content_block_text)
                .collect::<Vec<_>>()
                .join("\n");
            let prefix = if *is_error {
                "Tool failed"
            } else {
                "Tool completed"
            };
            Some(format!("{prefix}: {tool_name}\n{text}"))
        }
        ContentBlock::ProviderPayload { .. } => None,
    }
}

fn is_pipe_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.matches('|').count() >= 2
}

fn is_separator(line: &str) -> bool {
    split_pipe_row(line)
        .into_iter()
        .all(|cell| cell.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn split_pipe_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{escape_markdown_v2, render_markdown_v2, rewrite_pipe_tables};

    #[test]
    fn markdown_rendering_escapes_markdown_v2_control_chars() {
        assert_eq!(escape_markdown_v2("a_b *c*"), "a\\_b \\*c\\*");
    }

    #[test]
    fn markdown_rendering_keeps_fenced_code_blocks_readable() {
        let rendered = render_markdown_v2("before\n```rust\nlet x = 1;\n```\nafter");
        assert!(rendered.contains("```rust\nlet x = 1;\n```"));
        assert!(rendered.contains("before"));
    }

    #[test]
    fn markdown_rendering_rewrites_pipe_tables() {
        let rewritten = rewrite_pipe_tables("| A | B |\n|---|---|\n| x | y |");
        assert_eq!(rewritten, "- A: x, B: y");
    }
}

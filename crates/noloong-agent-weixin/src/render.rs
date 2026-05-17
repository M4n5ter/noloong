use crate::text::normalize_weixin_markdown;
use noloong_agent_core::{AgentMessage, ContentBlock};

pub fn render_agent_message_text(message: &AgentMessage) -> String {
    normalize_weixin_markdown(
        &message
            .content
            .iter()
            .filter_map(render_user_visible_content_block_text)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

pub fn render_user_visible_content_block_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text { text } => Some(text.clone()),
        ContentBlock::Json { value } => Some(value.to_string()),
        ContentBlock::Media { media } => {
            let name = media.name.as_deref().unwrap_or("media");
            Some(format!("[附件: {name}]"))
        }
        ContentBlock::Thinking { .. }
        | ContentBlock::ToolCall { .. }
        | ContentBlock::ToolResult { .. }
        | ContentBlock::ProviderPayload { .. } => None,
    }
}

pub fn render_content_block_text(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text { text } => Some(text.clone()),
        ContentBlock::Json { value } => Some(value.to_string()),
        ContentBlock::Media { media } => {
            let name = media.name.as_deref().unwrap_or("media");
            Some(format!("[附件: {name}]"))
        }
        ContentBlock::Thinking { .. } => None,
        ContentBlock::ToolCall { .. } => None,
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
                "工具失败"
            } else {
                "工具完成"
            };
            Some(format!("{prefix}: {tool_name}\n{text}"))
        }
        ContentBlock::ProviderPayload { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::render_agent_message_text;
    use noloong_agent_core::{AgentMessage, ContentBlock, ToolCall};

    #[test]
    fn skips_tool_call_blocks() {
        let message = AgentMessage::assistant(
            "assistant-1",
            vec![
                ContentBlock::ToolCall {
                    tool_call: ToolCall {
                        id: "tool-1".into(),
                        name: "host.exec.start".into(),
                        arguments: serde_json::json!({}),
                    },
                },
                ContentBlock::Text {
                    text: "done".into(),
                },
            ],
        );

        assert_eq!(render_agent_message_text(&message), "done");
    }

    #[test]
    fn skips_internal_tool_results_for_final_delivery() {
        let message = AgentMessage::assistant(
            "assistant-1",
            vec![ContentBlock::ToolResult {
                tool_call_id: "tool-1".into(),
                tool_name: "host.exec.start".into(),
                content: vec![ContentBlock::Json {
                    value: serde_json::json!({"internal": true}),
                }],
                is_error: false,
            }],
        );

        assert_eq!(render_agent_message_text(&message), "");
    }
}

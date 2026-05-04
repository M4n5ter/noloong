use super::hooks::PhaseHookRunner;
use super::{PhaseContext, PhaseOutput};
use crate::{
    AgentEffect, AgentMessage, ContentBlock, MediaBlock, MediaDelta, MediaSource, ModelStreamEvent,
    Result, ThinkingBlock,
};

pub(super) async fn assistant_commit(context: PhaseContext<'_>) -> Result<PhaseOutput> {
    let PhaseContext {
        runtime,
        run_id,
        turn_id,
        state,
        scratch,
        cancellation,
        ..
    } = context;
    let mut output = PhaseOutput::from_scratch(scratch);
    let hook_runner = PhaseHookRunner::new(
        runtime.phase_hooks(),
        run_id,
        turn_id,
        &state,
        &cancellation,
    );
    if hook_runner.has_hooks() {
        output.scratch.model_events = hook_runner
            .before_assistant_commit(output.scratch.model_events)
            .await?;
    }
    let mut thinking: Option<ThinkingBlock> = None;
    let mut media: Option<MediaBlock> = None;
    let mut text = String::new();
    let mut content = Vec::new();
    for event in &output.scratch.model_events {
        match event {
            ModelStreamEvent::ThinkingDelta { delta } => {
                flush_media(&mut content, &mut media);
                flush_text(&mut content, &mut text);
                if !delta.is_empty() {
                    if thinking
                        .as_ref()
                        .is_some_and(|block| block.kind != delta.kind)
                    {
                        flush_thinking(&mut content, &mut thinking);
                    }
                    match &mut thinking {
                        Some(block) => block.apply_delta(delta),
                        None => thinking = Some(ThinkingBlock::from_delta(delta)),
                    }
                }
            }
            ModelStreamEvent::TextDelta { text: delta } => {
                flush_thinking(&mut content, &mut thinking);
                flush_media(&mut content, &mut media);
                text.push_str(delta);
            }
            ModelStreamEvent::MediaDelta { delta } => {
                if delta.is_empty() {
                    continue;
                }
                flush_thinking(&mut content, &mut thinking);
                flush_text(&mut content, &mut text);
                if media
                    .as_ref()
                    .is_some_and(|block| media_delta_starts_new_block(block, delta))
                {
                    flush_media(&mut content, &mut media);
                }
                match &mut media {
                    Some(block) => block.apply_delta(delta),
                    None => media = MediaBlock::from_delta(delta),
                }
                if delta.done {
                    flush_media(&mut content, &mut media);
                }
            }
            ModelStreamEvent::ToolCall { tool_call } => {
                flush_thinking(&mut content, &mut thinking);
                flush_text(&mut content, &mut text);
                flush_media(&mut content, &mut media);
                content.push(ContentBlock::ToolCall {
                    tool_call: tool_call.clone(),
                });
            }
            ModelStreamEvent::Failed { error } => {
                return Err(crate::AgentCoreError::Phase(format!(
                    "model stream failed: {error}"
                )));
            }
            ModelStreamEvent::Started { .. } | ModelStreamEvent::Finished { .. } => {}
        }
    }
    flush_thinking(&mut content, &mut thinking);
    flush_text(&mut content, &mut text);
    flush_media(&mut content, &mut media);
    let message = AgentMessage::assistant(
        format!("assistant-{}-{}", context.run_id, context.turn_id),
        content,
    );
    let message = if hook_runner.has_hooks() {
        hook_runner.after_assistant_commit(message).await?
    } else {
        message
    };
    output.effects.push(AgentEffect::AppendMessage {
        message: message.clone(),
    });
    output.scratch.assistant_message = Some(message);
    Ok(output)
}

fn flush_thinking(content: &mut Vec<ContentBlock>, thinking: &mut Option<ThinkingBlock>) {
    if let Some(thinking) = thinking.take()
        && !thinking.is_empty()
    {
        content.push(ContentBlock::Thinking { thinking });
    }
}

fn flush_text(content: &mut Vec<ContentBlock>, text: &mut String) {
    if !text.is_empty() {
        content.push(ContentBlock::Text {
            text: std::mem::take(text),
        });
    }
}

fn flush_media(content: &mut Vec<ContentBlock>, media: &mut Option<MediaBlock>) {
    if let Some(media) = media.take() {
        content.push(ContentBlock::Media { media });
    }
}

fn media_delta_starts_new_block(block: &MediaBlock, delta: &MediaDelta) -> bool {
    if block.kind != delta.kind {
        return true;
    }
    let Some(source) = &delta.source else {
        return false;
    };
    if block.source == *source {
        return false;
    }
    !matches!(&block.source, MediaSource::Inline { .. })
}

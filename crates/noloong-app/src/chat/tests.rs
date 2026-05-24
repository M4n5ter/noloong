use super::{
    ChatApprovalStatus, ChatComposer, ChatComposerAction, ChatRunStatus, ChatSessionStore,
    ChatTranscriptItem, ChatTranscriptRole, StreamingText,
};
use crate::interaction::{
    AppContentBlock, AppDisplayEvent, AppInteractionSessionDescriptor, AppInteractionSessionState,
    AppInteractionSessionStatus, AppMediaKind, AppMediaSource, AppMessage, AppPromptInput,
    AppToolApprovalRequest, AppToolOutput, AppToolUpdate,
};
use std::fs;

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
    assert_eq!(store.transcript()[0].role(), ChatTranscriptRole::Assistant);
    assert_eq!(store.transcript()[0].text(), "hello");
    assert!(store.transcript()[0].streaming().is_some());

    store.apply_display_event(AppDisplayEvent::AssistantMessageFinal {
        run_id: "run-1".into(),
        display_message_id: "run-1:assistant".into(),
        message: message("assistant-1", "assistant", "hello!"),
        truncated: false,
    });

    assert_eq!(store.transcript().len(), 1);
    assert_eq!(store.transcript()[0].message_id, "assistant-1");
    assert_eq!(store.transcript()[0].text(), "hello!");
    assert_eq!(store.transcript()[0].streaming(), None);
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
fn composer_converts_file_attachment_to_media_message_input() {
    let dir = tempfile_dir("composer-attachment");
    let path = dir.join("notes.txt");
    fs::write(&path, "hello").unwrap();
    let mut composer = ChatComposer::default();
    composer.set_text("see attached".into());

    composer.add_attachment_path(&path).unwrap();

    let ChatComposerAction::Submit(submission) = composer.press_enter(false) else {
        panic!("expected submission");
    };
    let AppPromptInput::Message { message } = submission.into_prompt_input("user-1") else {
        panic!("attachment submission must use message input");
    };
    assert_eq!(message.id, "user-1");
    assert_eq!(message.role, "user");
    assert_eq!(
        message.content[0],
        AppContentBlock::Text {
            text: "see attached".into()
        }
    );
    let AppContentBlock::Media { media } = &message.content[1] else {
        panic!("expected media block");
    };
    assert_eq!(media.kind, AppMediaKind::File);
    assert_eq!(media.name.as_deref(), Some("notes.txt"));
    assert_eq!(media.mime_type.as_deref(), Some("text/plain"));
    assert_eq!(
        media.source,
        AppMediaSource::Uri {
            uri: format!("file://{}", path.display())
        }
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn composer_rejects_directories_and_keeps_draft_unchanged() {
    let dir = tempfile_dir("composer-unsupported-attachment");
    let mut composer = ChatComposer::default();

    assert!(composer.add_attachment_path(&dir).is_err());

    assert!(composer.attachments().is_empty());
    assert_eq!(composer.press_enter(false), ChatComposerAction::None);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn composer_removes_attachment_from_draft() {
    let dir = tempfile_dir("composer-attachment-remove");
    let path = dir.join("notes.txt");
    fs::write(&path, "hello").unwrap();
    let mut composer = ChatComposer::default();
    composer.add_attachment_path(&path).unwrap();
    let attachment_id = composer.attachments()[0].id.clone();

    assert!(composer.remove_attachment(&attachment_id));

    assert!(composer.attachments().is_empty());
    assert!(!composer.can_send());
    fs::remove_dir_all(dir).unwrap();
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
    assert_eq!(item.role(), ChatTranscriptRole::Thought);
    assert_eq!(item.text(), "summary");
    let thought = item.thought().expect("thought state");
    assert_eq!(thought.summary, "summary");
    assert_eq!(thought.raw, "raw detail");
    assert!(!thought.completed);

    store.apply_display_event(AppDisplayEvent::ThoughtCompleted {
        run_id: "run-1".into(),
        thought_id: "run-1:thought".into(),
        elapsed_ms: 2_000,
    });

    let item = &store.transcript()[0];
    assert_eq!(item.text(), "Thought for 2 seconds");
    let thought = item.thought().expect("thought state");
    assert!(thought.completed);
    assert_eq!(thought.elapsed_ms, Some(2_000));
    assert!(!thought.expanded);

    assert!(store.toggle_thought_expanded("run-1:thought"));
    assert!(store.transcript()[0].thought().unwrap().expanded);
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

#[test]
fn tool_display_events_aggregate_into_one_collapsed_activity() {
    let mut store = ChatSessionStore::default();
    store.refresh(vec![session_descriptor(
        "session-1",
        AppInteractionSessionStatus::Running,
        Vec::new(),
    )]);

    store.apply_display_event(AppDisplayEvent::ToolStarted {
        tool_call_id: "call-1".into(),
        tool_name: "host.exec.start".into(),
    });
    store.apply_display_event(AppDisplayEvent::ToolUpdated {
        tool_call_id: "call-1".into(),
        update: AppToolUpdate::text("stdout line\n"),
    });
    store.apply_display_event(AppDisplayEvent::ToolCompleted {
        tool_call_id: "call-1".into(),
        output: AppToolOutput::text("done"),
    });

    assert_eq!(store.transcript().len(), 1);
    let tool = store.transcript()[0].tool().expect("tool activity");
    assert_eq!(tool.tool_call_id, "call-1");
    assert_eq!(tool.tool_name, "host.exec.start");
    assert!(tool.completed);
    assert!(!tool.expanded);
    assert_eq!(tool.update_text(), "stdout line\n");
    assert_eq!(
        tool.output.as_ref().map(|output| output.text.as_str()),
        Some("done")
    );

    assert!(store.toggle_tool_expanded("call-1"));
    assert!(store.transcript()[0].tool().unwrap().expanded);
}

#[test]
fn final_session_descriptor_preserves_live_tool_activity() {
    let mut store = ChatSessionStore::default();
    store.refresh(vec![session_descriptor(
        "session-1",
        AppInteractionSessionStatus::Running,
        vec![message("user-1", "user", "run command")],
    )]);

    store.apply_display_event(AppDisplayEvent::ToolStarted {
        tool_call_id: "call-1".into(),
        tool_name: "host.exec.start".into(),
    });
    store.apply_display_event(AppDisplayEvent::ToolCompleted {
        tool_call_id: "call-1".into(),
        output: AppToolOutput::text("ok"),
    });

    store.upsert_and_select(session_descriptor(
        "session-1",
        AppInteractionSessionStatus::Completed,
        vec![
            message("user-1", "user", "run command"),
            message("assistant-1", "assistant", "done"),
        ],
    ));

    assert_eq!(
        store
            .transcript()
            .iter()
            .map(ChatTranscriptItem::role)
            .collect::<Vec<_>>(),
        vec![
            ChatTranscriptRole::User,
            ChatTranscriptRole::Tool,
            ChatTranscriptRole::Assistant,
        ]
    );
    let tool = store.transcript()[1].tool().expect("tool activity");
    assert_eq!(tool.tool_call_id, "call-1");
    assert_eq!(
        tool.output.as_ref().map(|output| output.text.as_str()),
        Some("ok")
    );
}

#[test]
fn local_user_message_is_visible_until_final_descriptor_reconciles_it() {
    let mut store = ChatSessionStore::default();
    store.refresh(vec![session_descriptor(
        "session-1",
        AppInteractionSessionStatus::Idle,
        Vec::new(),
    )]);

    assert!(store.append_local_user_message(
        "local-user-1",
        &AppPromptInput::Text {
            text: "hello".into(),
        },
    ));

    assert_eq!(store.transcript().len(), 1);
    assert_eq!(store.transcript()[0].message_id, "local-user-1");
    assert_eq!(store.transcript()[0].role(), ChatTranscriptRole::User);
    assert_eq!(store.transcript()[0].text(), "hello");

    store.upsert_and_select(session_descriptor(
        "session-1",
        AppInteractionSessionStatus::Completed,
        vec![
            message("user-1", "user", "hello"),
            message("assistant-1", "assistant", "done"),
        ],
    ));

    assert_eq!(
        store
            .transcript()
            .iter()
            .map(|item| (item.role(), item.message_id.as_str(), item.text()))
            .collect::<Vec<_>>(),
        vec![
            (ChatTranscriptRole::User, "user-1", "hello".into()),
            (ChatTranscriptRole::Assistant, "assistant-1", "done".into()),
        ]
    );
}

#[test]
fn long_tool_output_keeps_full_text_but_exposes_bounded_preview() {
    let mut store = ChatSessionStore::default();
    store.refresh(vec![session_descriptor(
        "session-1",
        AppInteractionSessionStatus::Running,
        Vec::new(),
    )]);
    let long_output = "x".repeat(5_000);

    store.apply_display_event(AppDisplayEvent::ToolCompleted {
        tool_call_id: "call-1".into(),
        output: AppToolOutput::text(long_output.clone()),
    });

    let tool = store.transcript()[0].tool().expect("tool activity");
    let output = tool.output.as_ref().expect("tool output");
    assert!(output.is_long());
    assert_eq!(
        tool.output.as_ref().map(|output| output.text.as_str()),
        Some(long_output.as_str())
    );
    assert!(output.preview_text().len() < long_output.len());
    assert!(output.preview_text().ends_with('…'));
}

#[test]
fn approval_request_renders_inline_card_and_paused_run_state() {
    let mut store = ChatSessionStore::default();
    store.refresh(vec![session_descriptor(
        "session-1",
        AppInteractionSessionStatus::Running,
        Vec::new(),
    )]);

    store.apply_display_event(AppDisplayEvent::RunPaused {
        run_id: "run-1".into(),
        reason: serde_json::json!({"type": "tool_approval"}),
    });
    store.apply_display_event(AppDisplayEvent::ApprovalRequested {
        approval: approval_request("approval-1", "host.exec.start"),
    });

    assert_eq!(
        store.current_run().map(|run| run.status),
        Some(ChatRunStatus::Paused)
    );
    assert!(!store.can_send_current_message());
    assert_eq!(store.transcript().len(), 1);
    let approval = store.transcript()[0].approval().expect("approval card");
    assert_eq!(approval.approval_id, "approval-1");
    assert_eq!(approval.tool_name, "host.exec.start");
    assert_eq!(approval.prompt.as_deref(), Some("Approve command?"));
    assert_eq!(approval.status, ChatApprovalStatus::Pending);
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

fn approval_request(approval_id: &str, tool_name: &str) -> AppToolApprovalRequest {
    AppToolApprovalRequest {
        approval_id: approval_id.into(),
        tool_call: crate::interaction::AppToolCall {
            id: "call-1".into(),
            name: tool_name.into(),
            arguments: serde_json::json!({"command": "echo ok"}),
        },
        permissions: Vec::new(),
        hook_id: None,
        request: crate::interaction::AppToolApprovalRequestSpec {
            prompt: Some("Approve command?".into()),
            reason: Some("Needs host command permission".into()),
            expires_at_ms: None,
            metadata: serde_json::json!({}),
        },
    }
}

fn tempfile_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("noloong-app-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

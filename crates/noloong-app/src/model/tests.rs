use super::{AppError, AppLaunchOptions, AppRoute, AppStatus, AppViewModel, ChatEmptyState};
use crate::chat::ChatTranscriptRole;
use crate::interaction::{
    AppContentBlock, AppDisplayEvent, AppInteractionClient, AppInteractionDisplayNotification,
    AppInteractionEndpoint, AppInteractionError, AppInteractionSessionDescriptor,
    AppInteractionSessionState, AppInteractionSessionStatus, AppInteractionStatus, AppMessage,
    AppPromptInput, AppPromptRequest, AppSessionCreateRequest, InteractionInitializeRequest,
    InteractionInitializeResult, InteractionProfileDescriptor, InteractionServerInfo,
    initialize_interaction_status,
};
use crate::test_support::{remove_temp_dir, temp_dir};
use noloong_config::Locale;
use std::fs;

#[test]
fn app_loads_starter_draft_when_config_is_missing() {
    let dir = temp_dir("app-missing-config");
    let path = dir.join("profile-config.jsonc");

    let model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    assert_eq!(model.locale, Locale::Zh);
    assert_eq!(model.route, AppRoute::Chat);
    assert_eq!(model.status, AppStatus::StarterDraft);
    assert_eq!(
        model.config.default_profile_id.as_deref(),
        Some("chatgpt-responses")
    );
    assert!(!path.exists());
    remove_temp_dir(dir);
}

#[test]
fn chat_empty_state_guides_missing_config_to_settings() {
    let dir = temp_dir("app-chat-missing-config");
    let path = dir.join("profile-config.jsonc");

    let model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    assert_eq!(model.chat_empty_state(), ChatEmptyState::MissingConfig);
    remove_temp_dir(dir);
}

#[test]
fn chat_empty_state_offers_new_session_after_runtime_initialize() {
    let dir = temp_dir("app-chat-ready-empty");
    let path = dir.join("profile-config.jsonc");

    let model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:12345/jsonrpc/ws".into(),
            bearer_token: Some("token".into()),
        }),
        interaction_status: Some(AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: Vec::new(),
        }),
    })
    .unwrap();

    assert_eq!(model.chat_empty_state(), ChatEmptyState::NoSession);
    remove_temp_dir(dir);
}

#[tokio::test]
async fn app_refreshes_real_sessions_and_recovers_current_transcript() {
    let dir = temp_dir("app-chat-refresh");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
            bearer_token: Some("secret".into()),
        }),
        interaction_status: Some(AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: Vec::new(),
        }),
    })
    .unwrap();
    let client = FakeInteractionClient::ok().with_sessions(vec![session_descriptor(
        "session-1",
        AppInteractionSessionStatus::Running,
        vec![
            message("user-1", "user", "请帮我检查项目"),
            message("assistant-1", "assistant", "我会先查看当前状态。"),
        ],
    )]);

    model.refresh_chat_sessions(&client).await.unwrap();

    assert_eq!(model.chat_sessions().len(), 1);
    assert_eq!(model.chat_sessions()[0].session_id, "session-1");
    assert_eq!(
        model.chat_sessions()[0].status,
        AppInteractionSessionStatus::Running
    );
    assert_eq!(model.current_chat_session_id(), Some("session-1"));
    let transcript = model.chat_transcript();
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0].role, ChatTranscriptRole::User);
    assert_eq!(transcript[0].text, "请帮我检查项目");
    assert_eq!(transcript[1].role, ChatTranscriptRole::Assistant);
    assert_eq!(transcript[1].text, "我会先查看当前状态。");
    remove_temp_dir(dir);
}

#[tokio::test]
async fn app_selects_chat_session_without_mutating_other_session_status() {
    let dir = temp_dir("app-chat-select-session");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
            bearer_token: Some("secret".into()),
        }),
        interaction_status: Some(AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: Vec::new(),
        }),
    })
    .unwrap();
    let client = FakeInteractionClient::ok().with_sessions(vec![
        session_descriptor(
            "session-running",
            AppInteractionSessionStatus::Running,
            vec![message("user-1", "user", "长任务继续跑")],
        ),
        session_descriptor(
            "session-completed",
            AppInteractionSessionStatus::Completed,
            vec![
                message("user-2", "user", "总结一下"),
                message("assistant-2", "assistant", "总结完成。"),
            ],
        ),
    ]);
    model.refresh_chat_sessions(&client).await.unwrap();

    assert!(model.select_chat_session("session-completed"));

    assert_eq!(model.current_chat_session_id(), Some("session-completed"));
    assert_eq!(
        model
            .chat_sessions()
            .iter()
            .find(|session| session.session_id == "session-running")
            .map(|session| session.status),
        Some(AppInteractionSessionStatus::Running)
    );
    assert_eq!(model.chat_transcript()[0].text, "总结一下");
    assert_eq!(model.chat_transcript()[1].text, "总结完成。");
    remove_temp_dir(dir);
}

#[tokio::test]
async fn app_creates_chat_session_through_interaction_client() {
    let dir = temp_dir("app-chat-create-session");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
            bearer_token: Some("secret".into()),
        }),
        interaction_status: Some(AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: Vec::new(),
        }),
    })
    .unwrap();
    let client = FakeInteractionClient::ok().with_create_session(session_descriptor(
        "session-new",
        AppInteractionSessionStatus::Idle,
        Vec::new(),
    ));

    model.create_chat_session(&client).await.unwrap();

    assert_eq!(model.current_chat_session_id(), Some("session-new"));
    assert_eq!(model.chat_sessions().len(), 1);
    assert_eq!(
        client
            .last_create_request()
            .and_then(|request| request.profile_id),
        Some("chatgpt-responses".into())
    );
    remove_temp_dir(dir);
}

#[tokio::test]
async fn app_refreshes_current_session_from_interaction_client() {
    let dir = temp_dir("app-chat-refresh-current");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
            bearer_token: Some("secret".into()),
        }),
        interaction_status: Some(AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: Vec::new(),
        }),
    })
    .unwrap();
    let client = FakeInteractionClient::ok()
        .with_sessions(vec![session_descriptor(
            "session-1",
            AppInteractionSessionStatus::Running,
            vec![message("user-1", "user", "开始")],
        )])
        .with_current_session(session_descriptor(
            "session-1",
            AppInteractionSessionStatus::Completed,
            vec![
                message("user-1", "user", "开始"),
                message("assistant-1", "assistant", "完成"),
            ],
        ));
    model.refresh_chat_sessions(&client).await.unwrap();

    model.refresh_current_chat_session(&client).await.unwrap();

    assert_eq!(
        model.chat_sessions()[0].status,
        AppInteractionSessionStatus::Completed
    );
    assert_eq!(model.chat_transcript()[1].text, "完成");
    assert_eq!(client.last_get_session_id().as_deref(), Some("session-1"));
    remove_temp_dir(dir);
}

#[tokio::test]
async fn first_chat_send_creates_session_then_submits_prompt() {
    let dir = temp_dir("app-chat-first-send");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
            bearer_token: Some("secret".into()),
        }),
        interaction_status: Some(AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: Vec::new(),
        }),
    })
    .unwrap();
    let client = FakeInteractionClient::ok()
        .with_create_session(session_descriptor(
            "session-new",
            AppInteractionSessionStatus::Idle,
            Vec::new(),
        ))
        .with_prompt_session(session_descriptor(
            "session-new",
            AppInteractionSessionStatus::Running,
            vec![message("user-1", "user", "hello")],
        ));

    model
        .submit_chat_message(&client, "  hello  ".into())
        .await
        .unwrap();

    assert_eq!(model.current_chat_session_id(), Some("session-new"));
    assert_eq!(
        model.chat_sessions()[0].status,
        AppInteractionSessionStatus::Running
    );
    assert_eq!(model.chat_transcript()[0].text, "hello");
    assert_eq!(
        client
            .last_prompt_request()
            .map(|request| (request.session_id, request.input)),
        Some((
            "session-new".into(),
            AppPromptInput::Text {
                text: "hello".into()
            }
        ))
    );
    remove_temp_dir(dir);
}

#[test]
fn app_applies_display_notifications_to_current_chat_session() {
    let dir = temp_dir("app-chat-display-notification");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:8787/jsonrpc/ws".into(),
            bearer_token: Some("secret".into()),
        }),
        interaction_status: Some(AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: Vec::new(),
        }),
    })
    .unwrap();
    model.apply_chat_session_descriptors(vec![session_descriptor(
        "session-1",
        AppInteractionSessionStatus::Running,
        Vec::new(),
    )]);

    model.apply_display_notification(AppInteractionDisplayNotification {
        session_id: "session-1".into(),
        subscription_id: "subscription-1".into(),
        event: AppDisplayEvent::AssistantMessageDelta {
            run_id: "run-1".into(),
            display_message_id: "run-1:assistant".into(),
            text: "hello".into(),
        },
    });
    model.apply_display_notification(AppInteractionDisplayNotification {
        session_id: "other-session".into(),
        subscription_id: "subscription-2".into(),
        event: AppDisplayEvent::AssistantMessageDelta {
            run_id: "run-2".into(),
            display_message_id: "run-2:assistant".into(),
            text: "ignored".into(),
        },
    });

    assert_eq!(model.chat_transcript().len(), 1);
    assert_eq!(model.chat_transcript()[0].text, "hello");
    remove_temp_dir(dir);
}

#[test]
fn app_loads_interaction_endpoint_for_chat_client() {
    let dir = temp_dir("app-interaction-endpoint");
    let path = dir.join("profile-config.jsonc");

    let model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::En),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:12345/jsonrpc/ws".into(),
            bearer_token: Some("token".into()),
        }),
        interaction_status: None,
    })
    .unwrap();

    assert_eq!(
        model
            .interaction_endpoint
            .as_ref()
            .map(|endpoint| endpoint.ws_url.as_str()),
        Some("ws://127.0.0.1:12345/jsonrpc/ws")
    );
    assert_eq!(
        model
            .interaction_endpoint
            .as_ref()
            .and_then(|endpoint| endpoint.bearer_token.as_deref()),
        Some("token")
    );
    remove_temp_dir(dir);
}

#[test]
fn app_loads_initial_interaction_status_from_launcher() {
    let dir = temp_dir("app-initial-interaction-status");
    let path = dir.join("profile-config.jsonc");

    let model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::En),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:12345/jsonrpc/ws".into(),
            bearer_token: Some("token".into()),
        }),
        interaction_status: Some(AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: Vec::new(),
        }),
    })
    .unwrap();

    assert_eq!(
        model.interaction_status,
        AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: Vec::new(),
        }
    );
    remove_temp_dir(dir);
}

#[test]
fn app_without_initial_status_waits_for_endpoint_initialize() {
    let dir = temp_dir("app-pending-interaction-status");
    let path = dir.join("profile-config.jsonc");

    let model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::En),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:12345/jsonrpc/ws".into(),
            bearer_token: Some("token".into()),
        }),
        interaction_status: None,
    })
    .unwrap();

    assert_eq!(model.interaction_status, AppInteractionStatus::Pending);
    remove_temp_dir(dir);
}

#[tokio::test]
async fn app_initializes_interaction_with_typed_client() {
    let dir = temp_dir("app-interaction-initialize");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::En),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:12345/jsonrpc/ws".into(),
            bearer_token: Some("token".into()),
        }),
        interaction_status: None,
    })
    .unwrap();

    let client = FakeInteractionClient::ok();
    model.interaction_status = initialize_interaction_status(&client).await;

    assert_eq!(
        client
            .last_request()
            .as_ref()
            .map(|request| request.name.as_str()),
        Some("noloong-app")
    );
    assert_eq!(
        model.interaction_status,
        AppInteractionStatus::Ready {
            server_name: "noloong-agent".into(),
            protocol_version: "2026-05-05".into(),
            profiles: vec![InteractionProfileDescriptor {
                profile_id: "default".into(),
                display_name: "Default".into(),
                description: None,
                default_manifest_patches: Vec::new(),
                metadata: Default::default(),
            }]
        }
    );
    remove_temp_dir(dir);
}

#[tokio::test]
async fn app_records_interaction_initialize_failure() {
    let dir = temp_dir("app-interaction-initialize-failure");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::En),
        interaction_endpoint: Some(AppInteractionEndpoint {
            ws_url: "ws://127.0.0.1:12345/jsonrpc/ws".into(),
            bearer_token: Some("token".into()),
        }),
        interaction_status: None,
    })
    .unwrap();

    let client = FakeInteractionClient::err("connection refused");
    model.interaction_status = initialize_interaction_status(&client).await;

    assert_eq!(
        model.interaction_status,
        AppInteractionStatus::Failed("connection refused".into())
    );
    remove_temp_dir(dir);
}

#[test]
fn app_saves_canonical_config() {
    let dir = temp_dir("app-save-config");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::En),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    model.set_display_name("Desktop Profile".into());
    model.save().unwrap();

    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"displayName\": \"Desktop Profile\""));
    assert_eq!(model.status, AppStatus::Saved);
    remove_temp_dir(dir);
}

#[test]
fn app_jsonc_preview_tracks_typed_draft() {
    let dir = temp_dir("app-jsonc-preview");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: None,
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    model.set_model("gpt-5.5".into());
    let preview = model.jsonc_preview().unwrap();

    assert!(preview.contains("\"model\": \"gpt-5.5\""));
    remove_temp_dir(dir);
}

#[test]
fn app_jsonc_editor_updates_typed_draft() {
    let dir = temp_dir("app-jsonc-editor-updates");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    let text = model
        .jsonc_preview()
        .unwrap()
        .replace("ChatGPT Responses", "JSONC Profile")
        .replace("gpt-5.4-mini", "gpt-5.5");

    assert!(model.set_jsonc_text(text));
    assert_eq!(
        model.selected_profile().unwrap().display_name,
        "JSONC Profile"
    );
    assert_eq!(model.model(), "gpt-5.5");
    assert_eq!(model.jsonc_error(), None);
    remove_temp_dir(dir);
}

#[test]
fn invalid_jsonc_does_not_pollute_typed_draft_and_blocks_save() {
    let dir = temp_dir("app-jsonc-invalid");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    let original_model = model.model();
    assert!(!model.set_jsonc_text("{ invalid".into()));

    assert_eq!(model.model(), original_model);
    assert!(model.is_settings_form_read_only());
    assert!(matches!(model.save(), Err(AppError::InvalidJsonc(_))));
    assert!(!path.exists());
    remove_temp_dir(dir);
}

#[test]
fn fixing_jsonc_restores_form_and_save_writes_canonical_json() {
    let dir = temp_dir("app-jsonc-fix");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    assert!(!model.set_jsonc_text("{ invalid".into()));
    let fixed = model
        .jsonc_preview()
        .unwrap()
        .replace("{ invalid", &model.config.to_canonical_json().unwrap());
    assert!(model.set_jsonc_text(fixed));
    assert!(!model.is_settings_form_read_only());

    model.save().unwrap();

    let saved = fs::read_to_string(&path).unwrap();
    assert_eq!(saved, model.config.to_canonical_json().unwrap());
    assert_eq!(model.jsonc_preview().unwrap(), saved);
    remove_temp_dir(dir);
}

#[test]
fn app_visual_mcp_editor_updates_typed_draft_and_jsonc() {
    let dir = temp_dir("app-mcp-editor");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    let index = model.add_mcp_stdio_server();
    model.set_mcp_server_id(index, "filesystem".into());
    model.set_mcp_endpoint(index, "npx".into());
    model.set_mcp_args(
        index,
        "@modelcontextprotocol/server-filesystem\n/tmp".into(),
    );
    model.set_mcp_tool_prefix(index, "fs".into());
    model.set_mcp_enabled_tools(index, "read_file, list_directory".into());
    model.set_mcp_timeout(index, "45".into());

    let edit = model.mcp_server_edit(index).unwrap();
    assert_eq!(edit.server_id, "filesystem");
    assert_eq!(edit.transport, "stdio");
    assert_eq!(edit.args, "@modelcontextprotocol/server-filesystem, /tmp");
    assert_eq!(edit.enabled_tools, "read_file, list_directory");
    assert_eq!(edit.request_timeout_secs, "45");

    let preview = model.jsonc_preview().unwrap();
    assert!(preview.contains("\"serverId\": \"filesystem\""));
    assert!(preview.contains("\"toolNamePrefix\": \"fs\""));
    assert!(preview.contains("\"requestTimeoutSecs\": 45"));
    remove_temp_dir(dir);
}

#[test]
fn app_visual_skills_editor_updates_typed_draft_and_jsonc() {
    let dir = temp_dir("app-skills-editor");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    let index = model.add_skill_root();
    model.set_skill_root(index, "/Users/example/.codex/skills".into());

    let edit = model.skill_root_edit(index).unwrap();
    assert_eq!(edit.root, "/Users/example/.codex/skills");
    assert_eq!(model.skill_root_summaries().len(), 1);

    let preview = model.jsonc_preview().unwrap();
    assert!(preview.contains("\"type\": \"skills\""));
    assert!(preview.contains("/Users/example/.codex/skills"));

    model.remove_skill_root(index);
    assert!(model.skill_root_summaries().is_empty());
    remove_temp_dir(dir);
}

#[test]
fn app_provider_switcher_manages_multiple_profiles() {
    let dir = temp_dir("app-provider-switcher");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    let added_id = model.add_provider_profile();
    model.set_display_name("Second Provider".into());
    model.set_model("gpt-5.5".into());
    model.activate_selected_provider_profile();

    assert_eq!(model.config.profiles.len(), 2);
    assert_eq!(
        model.config.default_profile_id.as_deref(),
        Some(added_id.as_str())
    );
    assert_eq!(model.model(), "gpt-5.5");

    let duplicated_id = model.duplicate_selected_provider_profile().unwrap();
    assert_ne!(duplicated_id, added_id);
    assert_eq!(model.config.profiles.len(), 3);
    assert_eq!(
        model.selected_profile().unwrap().display_name,
        "Second Provider Copy"
    );

    assert!(model.remove_selected_provider_profile());
    assert_eq!(model.config.profiles.len(), 2);
    assert!(model.config.validate().is_ok());
    assert!(model.jsonc_preview().unwrap().contains("\"profiles\""));
    remove_temp_dir(dir);
}

#[test]
fn app_visual_reasoning_and_compaction_editors_update_jsonc() {
    let dir = temp_dir("app-provider-detail-editors");
    let path = dir.join("profile-config.jsonc");
    let mut model = AppViewModel::load(AppLaunchOptions {
        profile_config_path: Some(path.display().to_string()),
        locale: Some(Locale::Zh),
        interaction_endpoint: None,
        interaction_status: None,
    })
    .unwrap();

    model.set_reasoning_effort("high");
    model.set_reasoning_summary("detailed");
    model.set_compaction_mode("openai_responses");
    model.set_compaction_input_limit_model("gpt-5.5".into());
    model.set_compaction_compact_model("gpt-5.4-mini".into());
    model.set_compaction_input_limit_tokens("272000".into());
    model.set_compaction_trigger_ratio("0.9".into());
    model.set_compaction_summary_budget_tokens("16384".into());
    model.set_compaction_keep_recent_tokens("20000".into());
    model.set_compaction_state_mode("persistent_state");
    model.set_compaction_timeout("120".into());

    let reasoning = model.reasoning_summary().unwrap();
    assert_eq!(reasoning.effort, "high");
    assert_eq!(reasoning.summary, "detailed");

    let compaction = model.compaction_edit();
    assert_eq!(compaction.mode, "openai_responses");
    assert_eq!(compaction.input_limit_model, "gpt-5.5");
    assert_eq!(compaction.trigger_ratio, "0.9");
    assert_eq!(compaction.state_mode, "persistent_state");

    let preview = model.jsonc_preview().unwrap();
    assert!(preview.contains("\"effort\": \"high\""));
    assert!(preview.contains("\"summary\": \"detailed\""));
    assert!(preview.contains("\"type\": \"openai_responses\""));
    assert!(preview.contains("\"inputLimitTokens\": 272000"));
    remove_temp_dir(dir);
}

struct FakeInteractionClient {
    result: Result<InteractionInitializeResult, AppInteractionError>,
    sessions: Vec<AppInteractionSessionDescriptor>,
    create_session: Option<AppInteractionSessionDescriptor>,
    prompt_session: Option<AppInteractionSessionDescriptor>,
    current_session: Option<AppInteractionSessionDescriptor>,
    last_request: std::sync::Mutex<Option<InteractionInitializeRequest>>,
    last_create_request: std::sync::Mutex<Option<AppSessionCreateRequest>>,
    last_prompt_request: std::sync::Mutex<Option<AppPromptRequest>>,
    last_get_session_id: std::sync::Mutex<Option<String>>,
}

impl FakeInteractionClient {
    fn ok() -> Self {
        Self {
            result: Ok(InteractionInitializeResult {
                server: InteractionServerInfo {
                    name: "noloong-agent".into(),
                    protocol_version: "2026-05-05".into(),
                },
                profiles: vec![InteractionProfileDescriptor {
                    profile_id: "default".into(),
                    display_name: "Default".into(),
                    description: None,
                    default_manifest_patches: Vec::new(),
                    metadata: Default::default(),
                }],
            }),
            sessions: Vec::new(),
            create_session: None,
            prompt_session: None,
            current_session: None,
            last_request: Default::default(),
            last_create_request: Default::default(),
            last_prompt_request: Default::default(),
            last_get_session_id: Default::default(),
        }
    }

    fn err(message: &str) -> Self {
        Self {
            result: Err(AppInteractionError::Transport(message.into())),
            sessions: Vec::new(),
            create_session: None,
            prompt_session: None,
            current_session: None,
            last_request: Default::default(),
            last_create_request: Default::default(),
            last_prompt_request: Default::default(),
            last_get_session_id: Default::default(),
        }
    }

    fn with_sessions(mut self, sessions: Vec<AppInteractionSessionDescriptor>) -> Self {
        self.sessions = sessions;
        self
    }

    fn with_current_session(mut self, descriptor: AppInteractionSessionDescriptor) -> Self {
        self.current_session = Some(descriptor);
        self
    }

    fn with_create_session(mut self, descriptor: AppInteractionSessionDescriptor) -> Self {
        self.create_session = Some(descriptor);
        self
    }

    fn with_prompt_session(mut self, descriptor: AppInteractionSessionDescriptor) -> Self {
        self.prompt_session = Some(descriptor);
        self
    }

    fn last_request(&self) -> Option<InteractionInitializeRequest> {
        self.last_request
            .lock()
            .expect("fake interaction lock")
            .clone()
    }

    fn last_create_request(&self) -> Option<AppSessionCreateRequest> {
        self.last_create_request
            .lock()
            .expect("fake interaction lock")
            .clone()
    }

    fn last_prompt_request(&self) -> Option<AppPromptRequest> {
        self.last_prompt_request
            .lock()
            .expect("fake interaction lock")
            .clone()
    }

    fn last_get_session_id(&self) -> Option<String> {
        self.last_get_session_id
            .lock()
            .expect("fake interaction lock")
            .clone()
    }
}

impl AppInteractionClient for FakeInteractionClient {
    async fn initialize(
        &self,
        request: InteractionInitializeRequest,
    ) -> Result<InteractionInitializeResult, AppInteractionError> {
        *self.last_request.lock().expect("fake interaction lock") = Some(request);
        self.result.clone()
    }

    async fn list_sessions(
        &self,
    ) -> Result<Vec<AppInteractionSessionDescriptor>, AppInteractionError> {
        Ok(self.sessions.clone())
    }

    async fn create_session(
        &self,
        request: AppSessionCreateRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        *self
            .last_create_request
            .lock()
            .expect("fake interaction lock") = Some(request);
        self.create_session
            .clone()
            .ok_or_else(|| AppInteractionError::Protocol("missing fake create session".into()))
    }

    async fn prompt(
        &self,
        request: AppPromptRequest,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        *self
            .last_prompt_request
            .lock()
            .expect("fake interaction lock") = Some(request);
        self.prompt_session
            .clone()
            .ok_or_else(|| AppInteractionError::Protocol("missing fake prompt session".into()))
    }

    async fn get_session(
        &self,
        session_id: &str,
    ) -> Result<AppInteractionSessionDescriptor, AppInteractionError> {
        *self
            .last_get_session_id
            .lock()
            .expect("fake interaction lock") = Some(session_id.into());
        self.current_session
            .clone()
            .ok_or_else(|| AppInteractionError::Protocol("missing fake current session".into()))
    }
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

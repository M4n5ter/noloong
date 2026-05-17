use super::{Cli, CliCommand, CliError, validate_interaction_bind};
use crate::build_info_cli::{BuildInfoSourceSubcommand, BuildInfoSubcommand};
use crate::cli::profile_locale;
use crate::config::HostProfileConfig;
use crate::profile_config_cli::{
    ProfileConfigSchemaOptions, ProfileConfigSubcommand, run_profile_config_schema,
};
use crate::schema::profile_config_schema_json;
use crate::telegram_cli::{
    BridgeUpdateHandler, TelegramBridgeOptions, apply_profile_media_fallback_policy,
    profile_media_fallback_policy, register_telegram_commands, telegram_config_from_values,
};
use crate::test_support::{remove_temp_file, write_temp_file};
use crate::weixin_cli::{WeixinBridgeOptions, WeixinSubcommand, weixin_config_from_values};
use clap::Parser;
use noloong_agent::{
    AgentManifest, JobSnapshot, JobStatus, Locale, ManifestPatch, ManifestPatchProposal,
    OutputChunk, ProcessOutput, ProcessOutputStream, SystemPromptAddition, WaitOutcome,
    interaction::{
        InteractionClientError, InteractionProfileDescriptor, InteractionSessionDescriptor,
        InteractionSessionStatus, InteractionWsNotification,
    },
};
use noloong_agent_core::{
    AgentMessage, AgentState, QueueMode, ToolApprovalRequest, ToolApprovalRequestSpec, ToolCall,
};
use noloong_agent_telegram::{
    access::{TelegramChatKind, TelegramTextInput},
    bridge::{TelegramBridgeError, TelegramInteractionClient, TelegramInteractionFuture},
    config::{TelegramFilePolicy, TelegramNativeMediaDecision, TelegramNativeMediaHandling},
    delivery::{TelegramDelivery, TelegramMessageTarget},
    display::{TelegramDisplayState, deliver_display_event},
    i18n::TelegramUiCatalog,
    input::{TelegramCommand, TelegramInboundContext, TelegramInboundUpdate},
    media::TelegramAttachmentResolver,
    polling::{TelegramCallbackQuery, TelegramChat, TelegramMessage, TelegramUpdate, TelegramUser},
    process::{PROCESS_OUTPUT_INLINE_CHAR_LIMIT, process_output_read_max_bytes},
    queue::{TelegramQueueKind, TelegramQueuedMessage, TelegramQueuedMessageIntent},
    session::{TelegramSessionActionStore, telegram_session_metadata},
    telegram_api::{
        TelegramApi, TelegramApiError, TelegramDeleteMessageRequest,
        TelegramEditMessageTextRequest, TelegramMessageHandle, TelegramSendDocumentRequest,
        TelegramSendMessageRequest, TelegramSetMyCommandsRequest,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    future::Future,
    net::SocketAddr,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::{Mutex, broadcast};

#[test]
fn cli_serve_rejects_public_bind_without_token() {
    let bind: SocketAddr = "0.0.0.0:8787".parse().unwrap();

    let error = validate_interaction_bind(bind, None).unwrap_err();

    assert!(matches!(error, CliError::PublicBindWithoutToken(_)));
}

#[test]
fn cli_telegram_bridge_requires_interaction_url() {
    let options = TelegramBridgeOptions {
        bot_token: Some("token".into()),
        allowed_users: Some("123456789".into()),
        ..Default::default()
    };

    let error = telegram_config_from_values(&options, |_| None).unwrap_err();

    assert!(error.to_string().contains("NOLOONG_INTERACTION_URL"));
}

#[test]
fn cli_telegram_bridge_requires_allowlist() {
    let options = TelegramBridgeOptions {
        interaction_url: Some("ws://127.0.0.1:8787/jsonrpc/ws".into()),
        bot_token: Some("token".into()),
        ..Default::default()
    };

    let error = telegram_config_from_values(&options, |_| None).unwrap_err();

    assert!(error.to_string().contains("allowlist"));
}

#[test]
fn cli_telegram_embeds_loopback_interaction_options() {
    let cli = Cli::try_parse_from([
        "noloong",
        "telegram",
        "--profile-config",
        "profiles.json",
        "--telegram-bot-username",
        "noloong_bot",
        "--telegram-allowed-users",
        "123456789",
        "--telegram-locale",
        "zh",
    ])
    .unwrap();

    let CliCommand::Telegram(options) = cli.command else {
        panic!("expected telegram command");
    };
    assert_eq!(options.profile_config.as_deref(), Some("profiles.json"));
    assert_eq!(options.bridge.bot_username.as_deref(), Some("noloong_bot"));
    assert_eq!(options.bridge.allowed_users.as_deref(), Some("123456789"));
    assert_eq!(options.bridge.locale, Some(Locale::Zh));
}

#[test]
fn cli_weixin_run_embeds_loopback_interaction_options() {
    let cli = Cli::try_parse_from([
        "noloong",
        "weixin",
        "run",
        "--profile-config",
        "profiles.json",
        "--weixin-account-id",
        "wx-bot",
        "--weixin-allowed-users",
        "user-1,user-2",
        "--weixin-locale",
        "zh",
    ])
    .unwrap();

    let CliCommand::Weixin(command) = cli.command else {
        panic!("expected weixin command");
    };
    let WeixinSubcommand::Run(options) = command.command else {
        panic!("expected weixin run");
    };
    assert_eq!(options.profile_config.as_deref(), Some("profiles.json"));
    assert_eq!(options.bridge.account_id.as_deref(), Some("wx-bot"));
    assert_eq!(
        options.bridge.allowed_users.as_deref(),
        Some("user-1,user-2")
    );
    assert_eq!(options.bridge.locale, Some(Locale::Zh));
}

#[test]
fn cli_profile_config_schema_command_parses() {
    let cli = Cli::try_parse_from([
        "noloong",
        "profile-config",
        "schema",
        "--check",
        "schemas/profile-config.schema.json",
    ])
    .unwrap();

    let CliCommand::ProfileConfig(command) = cli.command else {
        panic!("expected profile-config command");
    };
    let ProfileConfigSubcommand::Schema(options) = command.command;
    assert_eq!(
        options.check,
        Some(PathBuf::from("schemas/profile-config.schema.json"))
    );
}

#[test]
fn cli_build_info_commands_parse() {
    let manifest = Cli::try_parse_from(["noloong", "build-info", "manifest"]).unwrap();
    let CliCommand::BuildInfo(command) = manifest.command else {
        panic!("expected build-info command");
    };
    assert!(matches!(command.command, BuildInfoSubcommand::Manifest));

    let list = Cli::try_parse_from(["noloong", "build-info", "source", "list"]).unwrap();
    let CliCommand::BuildInfo(command) = list.command else {
        panic!("expected build-info command");
    };
    let BuildInfoSubcommand::Source(source) = command.command else {
        panic!("expected build-info source command");
    };
    assert!(matches!(source.command, BuildInfoSourceSubcommand::List));

    let cat =
        Cli::try_parse_from(["noloong", "build-info", "source", "cat", "Cargo.toml"]).unwrap();
    let CliCommand::BuildInfo(command) = cat.command else {
        panic!("expected build-info command");
    };
    let BuildInfoSubcommand::Source(source) = command.command else {
        panic!("expected build-info source command");
    };
    let BuildInfoSourceSubcommand::Cat(options) = source.command else {
        panic!("expected build-info source cat command");
    };
    assert_eq!(options.path, "Cargo.toml");

    let extract = Cli::try_parse_from([
        "noloong",
        "build-info",
        "source",
        "extract",
        "--output-dir",
        "out",
        "--force",
    ])
    .unwrap();
    let CliCommand::BuildInfo(command) = extract.command else {
        panic!("expected build-info command");
    };
    let BuildInfoSubcommand::Source(source) = command.command else {
        panic!("expected build-info source command");
    };
    let BuildInfoSourceSubcommand::Extract(options) = source.command else {
        panic!("expected build-info source extract command");
    };
    assert_eq!(options.output_dir, PathBuf::from("out"));
    assert!(options.force);

    let archive = Cli::try_parse_from([
        "noloong",
        "build-info",
        "source",
        "archive",
        "--output",
        "source.tar.zst",
    ])
    .unwrap();
    let CliCommand::BuildInfo(command) = archive.command else {
        panic!("expected build-info command");
    };
    let BuildInfoSubcommand::Source(source) = command.command else {
        panic!("expected build-info source command");
    };
    let BuildInfoSourceSubcommand::Archive(options) = source.command else {
        panic!("expected build-info source archive command");
    };
    assert_eq!(options.output, PathBuf::from("source.tar.zst"));
}

#[test]
fn cli_profile_config_schema_rejects_output_and_check_together() {
    let error = Cli::try_parse_from([
        "noloong",
        "profile-config",
        "schema",
        "--output",
        "schemas/profile-config.schema.json",
        "--check",
        "schemas/profile-config.schema.json",
    ])
    .unwrap_err();

    assert!(error.to_string().contains("cannot be used with"));
}

#[test]
fn profile_config_schema_check_accepts_matching_file() {
    let path = write_temp_file("profile-schema", "json", &profile_config_schema_json());

    run_profile_config_schema(ProfileConfigSchemaOptions {
        check: Some(path.clone()),
        ..Default::default()
    })
    .unwrap();
    remove_temp_file(path);
}

#[test]
fn profile_config_schema_check_rejects_mismatch() {
    let path = write_temp_file("profile-schema-mismatch", "json", "{}\n");

    let error = run_profile_config_schema(ProfileConfigSchemaOptions {
        check: Some(path.clone()),
        ..Default::default()
    })
    .unwrap_err();
    remove_temp_file(path);

    assert!(error.to_string().contains("schema is out of date"));
}

#[test]
fn telegram_text_input_detects_reply_to_bot() {
    let message = TelegramMessage {
        message_id: 2,
        message_thread_id: None,
        chat: TelegramChat {
            id: -100,
            kind: "supergroup".into(),
        },
        from: Some(TelegramUser {
            id: 7,
            username: Some("alice".into()),
        }),
        text: Some("continue".into()),
        caption: None,
        entities: Vec::new(),
        caption_entities: Vec::new(),
        photo: Vec::new(),
        document: None,
        audio: None,
        voice: None,
        video: None,
        reply_to_message: Some(Box::new(TelegramMessage {
            message_id: 1,
            message_thread_id: None,
            chat: TelegramChat {
                id: -100,
                kind: "supergroup".into(),
            },
            from: Some(TelegramUser {
                id: 1,
                username: Some("Noloong_Bot".into()),
            }),
            text: Some("previous".into()),
            caption: None,
            entities: Vec::new(),
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        })),
    };

    let input = match TelegramInboundUpdate::from_message(message, Some("@noloong_bot")).unwrap() {
        TelegramInboundUpdate::Message(message) => message.into_text_input().unwrap(),
        TelegramInboundUpdate::Command(_) => panic!("expected text input"),
    };

    assert!(input.is_reply_to_bot);
}

#[test]
fn telegram_config_uses_env_values() {
    let env = BTreeMap::from([
        ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
        ("TELEGRAM_BOT_TOKEN", "token"),
        ("TELEGRAM_BOT_USERNAME", "noloong_bot"),
        ("TELEGRAM_ALLOWED_USERS", "123456789"),
        ("TELEGRAM_LOCALE", "zh"),
    ]);

    let config = telegram_config_from_values(&TelegramBridgeOptions::default(), |name| {
        env.get(name).map(|value| value.to_string())
    })
    .unwrap();

    assert!(config.access.allows(1, Some(123456789)));
    assert_eq!(config.bot_username.as_deref(), Some("noloong_bot"));
    assert_eq!(config.locale, Locale::Zh);
}

#[test]
fn weixin_config_uses_env_values() {
    let env = BTreeMap::from([
        ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
        ("WEIXIN_ACCOUNT_ID", "wx-bot"),
        ("WEIXIN_TOKEN", "token"),
        ("WEIXIN_ALLOWED_USERS", "user-1,user-2"),
        ("WEIXIN_LOCALE", "zh"),
    ]);

    let config = weixin_config_from_values(&WeixinBridgeOptions::default(), |name| {
        env.get(name).map(|value| value.to_string())
    })
    .unwrap();

    assert_eq!(config.account_id, "wx-bot");
    assert!(config.access.allows_dm("user-1"));
    assert!(!config.access.allows_dm("user-3"));
    assert_eq!(config.locale, Locale::Zh);
}

#[test]
fn weixin_embedded_can_inherit_profile_locale() {
    let config =
        HostProfileConfig::load("examples/profile-configs/weixin-chatgpt-subscription.json")
            .unwrap();

    assert_eq!(profile_locale(&config, None), Some(Locale::Zh));
}

#[test]
fn weixin_config_rejects_missing_allowlist() {
    let env = BTreeMap::from([
        ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
        ("WEIXIN_ACCOUNT_ID", "wx-bot"),
        ("WEIXIN_TOKEN", "token"),
    ]);

    let error = weixin_config_from_values(&WeixinBridgeOptions::default(), |name| {
        env.get(name).map(|value| value.to_string())
    })
    .unwrap_err();

    assert!(error.to_string().contains("allowlist"));
}

#[test]
fn telegram_config_rejects_invalid_locale_env() {
    let env = BTreeMap::from([
        ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
        ("TELEGRAM_BOT_TOKEN", "token"),
        ("TELEGRAM_ALLOWED_USERS", "123456789"),
        ("TELEGRAM_LOCALE", "fr"),
    ]);

    let error = telegram_config_from_values(&TelegramBridgeOptions::default(), |name| {
        env.get(name).map(|value| value.to_string())
    })
    .unwrap_err();

    assert!(error.to_string().contains("invalid locale"));
}

#[test]
fn telegram_config_ignores_empty_proxy_env() {
    let env = BTreeMap::from([
        ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
        ("TELEGRAM_BOT_TOKEN", "token"),
        ("TELEGRAM_ALLOWED_USERS", "123456789"),
        ("TELEGRAM_PROXY", ""),
    ]);

    let config = telegram_config_from_values(&TelegramBridgeOptions::default(), |name| {
        env.get(name).map(|value| value.to_string())
    })
    .unwrap();

    assert_eq!(config.network.proxy_url, None);
}

#[test]
fn telegram_config_parses_manual_unsupported_media_fallback() {
    let env = BTreeMap::from([
        ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
        ("TELEGRAM_BOT_TOKEN", "token"),
        ("TELEGRAM_ALLOWED_USERS", "123456789"),
        ("TELEGRAM_UNSUPPORTED_MEDIA_FALLBACK_TO_FILE", "audio,video"),
    ]);

    let config = telegram_config_from_values(&TelegramBridgeOptions::default(), |name| {
        env.get(name).map(|value| value.to_string())
    })
    .unwrap();

    assert_eq!(
        config.file_policy.unsupported_media_fallback.audio,
        TelegramNativeMediaHandling::File
    );
    assert_eq!(
        config.file_policy.unsupported_media_fallback.voice,
        TelegramNativeMediaHandling::Native
    );
    assert_eq!(
        config.file_policy.unsupported_media_fallback.video,
        TelegramNativeMediaHandling::File
    );
}

#[test]
fn telegram_embedded_mode_derives_media_fallback_from_profile_provider() {
    let config = serde_json::from_value::<HostProfileConfig>(json!({
        "defaultProfileId": "chatgpt",
        "profiles": [
            {
                "profileId": "chatgpt",
                "displayName": "ChatGPT",
                "provider": {
                    "type": "chatgpt_responses",
                    "model": "gpt-5.4-mini",
                    "allowFileDataUrlInput": true
                }
            }
        ]
    }))
    .unwrap();
    let mut file_policy = TelegramFilePolicy::default();

    apply_profile_media_fallback_policy(&mut file_policy, &config, None);

    let fallback = file_policy.unsupported_media_fallback;
    assert_eq!(
        fallback.audio.decision_for_mime_type("application/pdf"),
        TelegramNativeMediaDecision::File
    );
    assert_eq!(
        fallback.audio.decision_for_mime_type("audio/ogg"),
        TelegramNativeMediaDecision::Unsupported
    );
    assert_eq!(
        fallback.video.decision_for_mime_type("video/mp4"),
        TelegramNativeMediaDecision::Unsupported
    );
}

#[test]
fn telegram_chat_completions_fallback_keeps_supported_audio_native() {
    let config = serde_json::from_value::<HostProfileConfig>(json!({
        "profiles": [
            {
                "profileId": "chat",
                "displayName": "Chat",
                "provider": {
                    "type": "chat_completions",
                    "model": "openrouter/free"
                }
            }
        ]
    }))
    .unwrap();

    let policy = profile_media_fallback_policy(&config, None);

    assert_eq!(
        policy.audio.decision_for_mime_type("audio/mpeg"),
        TelegramNativeMediaDecision::Native
    );
    assert_eq!(
        policy.audio.decision_for_mime_type("audio/ogg"),
        TelegramNativeMediaDecision::Unsupported
    );
    assert_eq!(policy.video, TelegramNativeMediaHandling::Native);
}

#[tokio::test]
async fn telegram_callback_resolves_approval_and_deletes_card() {
    let fixture = TelegramCallbackFixture::new().await;

    fixture.handle_callback("cb-1", 621).await.unwrap();
    fixture.handle_callback("cb-2", 621).await.unwrap();

    assert_eq!(fixture.interaction.methods(), vec!["approval/resolve"]);
    assert_eq!(
        fixture.api.answered_texts(),
        vec![Some("Recorded".into()), Some("Approval expired".into())]
    );
    assert_eq!(fixture.api.deleted_message_ids(), vec![10]);
    assert!(fixture.api.edited_reply_markup().is_empty());
}

#[tokio::test]
async fn telegram_callback_rejects_unauthorized_without_consuming_approval() {
    let fixture = TelegramCallbackFixture::new().await;

    fixture.handle_callback("cb-1", 999).await.unwrap();
    fixture.handle_callback("cb-2", 621).await.unwrap();

    assert_eq!(fixture.interaction.methods(), vec!["approval/resolve"]);
    assert_eq!(
        fixture.api.answered_texts(),
        vec![Some("Not allowed".into()), Some("Recorded".into())]
    );
}

#[tokio::test]
async fn telegram_approvals_command_lists_runtime_approvals() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;
    fixture.interaction.set_approval_list(BTreeMap::from([(
        "approval-1".into(),
        approval_request("approval-1"),
    )]));

    fixture
        .handler
        .handle_command(approvals_command())
        .await
        .unwrap();

    assert_eq!(
        fixture.api.sent_texts().last().unwrap(),
        "Pending approvals: 1\n1\\. \\`host\\_exec\\` \\(approval\\-1\\)"
    );
    assert!(
        fixture
            .interaction
            .methods()
            .into_iter()
            .any(|method| method == "approval/list")
    );
}

#[tokio::test]
async fn telegram_approve_command_resolves_only_pending_approval() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;
    fixture.interaction.set_approval_list(BTreeMap::from([(
        "approval-1".into(),
        approval_request("approval-1"),
    )]));

    fixture
        .handler
        .handle_command(telegram_command(22, "approve"))
        .await
        .unwrap();

    let calls = fixture.interaction.calls();
    let (_, params) = calls
        .iter()
        .find(|(method, _)| method == "approval/resolve")
        .unwrap();
    assert_eq!(params["approvalId"], "approval-1");
    assert_eq!(params["decision"]["outcome"], "allow");
    assert_eq!(fixture.api.deleted_message_ids(), vec![10]);
    assert!(fixture.api.edited_texts().is_empty());
}

#[tokio::test]
async fn telegram_deny_command_resolves_selected_pending_approval() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;
    fixture.interaction.set_approval_list(BTreeMap::from([
        ("approval-1".into(), approval_request("approval-1")),
        ("approval-2".into(), approval_request("approval-2")),
    ]));

    fixture
        .handler
        .handle_command(telegram_command_with_args(23, "deny", "2"))
        .await
        .unwrap();

    let calls = fixture.interaction.calls();
    let (_, params) = calls
        .iter()
        .find(|(method, _)| method == "approval/resolve")
        .unwrap();
    assert_eq!(params["approvalId"], "approval-2");
    assert_eq!(params["decision"]["outcome"], "deny");
    assert_eq!(
        fixture.api.sent_texts().last().unwrap(),
        "Approval resolved: deny"
    );
}

#[tokio::test]
async fn telegram_registers_command_menu_payload() {
    let api = Arc::new(FakeTelegramApi::default());

    register_telegram_commands(api.as_ref(), TelegramUiCatalog::new(Locale::Zh))
        .await
        .unwrap();

    let requests = api.command_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].language_code, None);
    assert!(requests[0].commands.iter().any(|command| {
        command.command == "approvals" && command.description == "列出待处理审批"
    }));
    assert_eq!(requests[0].commands.len(), 18);
}

#[tokio::test]
async fn telegram_unknown_command_returns_help_without_prompt() {
    let fixture = TelegramCallbackFixture::new().await;

    fixture
        .handler
        .handle_command(unknown_command())
        .await
        .unwrap();

    assert!(fixture.interaction.methods().is_empty());
    let sent_texts = fixture.api.sent_texts();
    let text = sent_texts.last().unwrap();
    assert!(text.contains("Unknown command"));
    assert!(text.contains("/approvals"));
}

#[tokio::test]
async fn telegram_known_future_command_returns_stub_without_prompt() {
    let fixture = TelegramCallbackFixture::new().await;

    fixture
        .handler
        .handle_command(telegram_command(5, "settings"))
        .await
        .unwrap();

    assert!(fixture.interaction.methods().is_empty());
    assert_eq!(
        fixture.api.sent_texts().last().unwrap(),
        "/settings is in the cockpit menu\\. Its control surface is not implemented yet\\."
    );
}

#[tokio::test]
async fn telegram_profiles_command_lists_profiles_and_selects_default() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.interaction.set_profiles(vec![
        profile_descriptor("profile-1"),
        profile_descriptor("profile-2"),
    ]);

    fixture
        .handler
        .handle_command(telegram_command(6, "profiles"))
        .await
        .unwrap();
    let callback_data = fixture.api.last_sent_callback_data(1, 0);
    fixture
        .handle_callback_with_data("profile-cb", 621, &callback_data)
        .await
        .unwrap();
    fixture
        .handler
        .handle_command(telegram_command(7, "new"))
        .await
        .unwrap();

    let calls = fixture.interaction.calls();
    assert!(calls.iter().any(|(method, _)| method == "profile/list"));
    assert!(calls.iter().any(|(method, params)| {
        method == "session/create" && params["profileId"] == "profile-2"
    }));
    assert!(
        fixture
            .api
            .edited_texts()
            .iter()
            .any(|text| text.contains("Default profile selected"))
    );
}

#[tokio::test]
async fn telegram_sessions_command_switches_and_confirms_delete() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.interaction.set_sessions(vec![
        telegram_session_descriptor("telegram:42"),
        telegram_session_descriptor_with_status(
            "telegram:42:session:9",
            InteractionSessionStatus::Running,
        ),
    ]);

    fixture
        .handler
        .handle_command(telegram_command(8, "sessions"))
        .await
        .unwrap();
    let switch_data = fixture.api.last_sent_callback_data(1, 0);
    let delete_data = fixture.api.last_sent_callback_data(1, 1);
    fixture
        .handle_callback_with_data("switch-cb", 621, &switch_data)
        .await
        .unwrap();

    fixture
        .handler
        .handle_command(telegram_command(9, "sessions"))
        .await
        .unwrap();
    fixture
        .handle_callback_with_data("delete-cb", 621, &delete_data)
        .await
        .unwrap();
    let confirm_data = fixture.api.edited_callback_data(0, 0);
    fixture
        .handle_callback_with_data("confirm-cb", 621, &confirm_data)
        .await
        .unwrap();
    fixture
        .handle_callback_with_data("confirm-cb-repeat", 621, &confirm_data)
        .await
        .unwrap();

    let calls = fixture.interaction.calls();
    assert!(calls.iter().any(|(method, params)| {
        method == "session/list"
            && params["metadataEquals"]["channel"] == "telegram"
            && params["metadataEquals"]["chatId"] == 42
    }));
    assert!(calls.iter().any(|(method, params)| {
        method == "session/get" && params["sessionId"] == "telegram:42:session:9"
    }));
    assert!(calls.iter().any(|(method, params)| {
        method == "session/delete"
            && params["sessionId"] == "telegram:42:session:9"
            && params["forceAbort"] == true
    }));
    assert_eq!(
        calls
            .iter()
            .filter(|(method, _)| method == "session/delete")
            .count(),
        1
    );
    assert!(
        fixture
            .api
            .answered_texts()
            .iter()
            .any(|text| text.as_deref() == Some("Action expired"))
    );
}

#[tokio::test]
async fn telegram_status_command_reads_active_session() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;

    fixture
        .handler
        .handle_command(telegram_command(10, "status"))
        .await
        .unwrap();

    assert!(
        fixture
            .interaction
            .calls()
            .iter()
            .any(|(method, _)| method == "session/get")
    );
    assert!(
        fixture
            .api
            .sent_texts()
            .last()
            .unwrap()
            .contains("Active session")
    );
}

#[tokio::test]
async fn telegram_continue_command_calls_agent_continue() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;

    fixture
        .handler
        .handle_command(telegram_command(11, "continue"))
        .await
        .unwrap();

    assert!(fixture.interaction.calls().iter().any(|(method, params)| {
        method == "agent/continue" && params["sessionId"] == "telegram:42"
    }));
    assert_eq!(
        fixture.api.sent_texts().last().unwrap(),
        "Run continued\nSession: telegram:42"
    );
}

#[tokio::test]
async fn telegram_abort_command_confirms_running_session() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;
    fixture
        .interaction
        .set_session_status("telegram:42", InteractionSessionStatus::Running);

    fixture
        .handler
        .handle_command(telegram_command(12, "abort"))
        .await
        .unwrap();
    let callback_data = fixture.api.last_sent_callback_data(0, 0);
    assert!(
        !fixture
            .interaction
            .methods()
            .iter()
            .any(|method| method == "agent/abort")
    );

    fixture
        .handle_callback_with_data("abort-cb", 621, &callback_data)
        .await
        .unwrap();

    assert!(fixture.interaction.calls().iter().any(|(method, params)| {
        method == "agent/abort" && params["sessionId"] == "telegram:42"
    }));
    assert_eq!(
        fixture.api.edited_texts().last().unwrap(),
        "Run aborted\nSession: telegram:42"
    );
}

#[tokio::test]
async fn telegram_queue_command_lists_and_controls_queues() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;
    fixture.interaction.set_queue(
        "telegram:42",
        TelegramQueueKind::Steering,
        vec![queued_user_message("queued-steer", "queued steering")],
    );

    fixture
        .handler
        .handle_command(telegram_command(13, "queue"))
        .await
        .unwrap();
    let clear_data = fixture.api.last_sent_callback_data(0, 0);
    let set_mode_data = fixture.api.last_sent_callback_data(1, 0);

    assert_eq!(
        fixture.api.sent_texts().last().unwrap(),
        "Queues: 1\nSteering: 1\n  1\\. user input: queued steering\nFollow\\-up: 0\n  empty"
    );

    fixture
        .handle_callback_with_data("queue-clear-cb", 621, &clear_data)
        .await
        .unwrap();
    fixture
        .handle_callback_with_data("queue-mode-cb", 621, &set_mode_data)
        .await
        .unwrap();

    let calls = fixture.interaction.calls();
    assert!(
        calls
            .iter()
            .any(|(method, params)| { method == "queue/list" && params["queue"] == "steering" })
    );
    assert!(
        calls
            .iter()
            .any(|(method, params)| { method == "queue/clear" && params["queue"] == "steering" })
    );
    assert!(calls.iter().any(|(method, params)| {
        method == "queue/set_mode" && params["queue"] == "steering" && params["mode"] == "all"
    }));
}

#[tokio::test]
async fn telegram_queue_command_with_args_adds_follow_up() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;

    fixture
        .handler
        .handle_command(telegram_command_with_args(14, "queue", "use this next"))
        .await
        .unwrap();

    assert!(fixture.interaction.calls().iter().any(|(method, params)| {
        method == "agent/follow_up"
            && params["sessionId"] == "telegram:42"
            && params["message"]["content"][0]["text"] == "use this next"
    }));
    assert_eq!(
        fixture.api.sent_texts().last().unwrap(),
        "Follow\\-up queued\nSession: telegram:42"
    );
}

#[tokio::test]
async fn telegram_processes_command_lists_and_opens_job() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;
    fixture
        .interaction
        .set_processes(vec![job_snapshot("job-1", JobStatus::Running)]);
    fixture
        .interaction
        .set_process_output(process_output("job-1", "hello"));

    fixture
        .handler
        .handle_command(telegram_command(15, "processes"))
        .await
        .unwrap();
    let open_data = fixture.api.last_sent_callback_data(0, 0);
    fixture
        .handle_callback_with_data("process-open-cb", 621, &open_data)
        .await
        .unwrap();

    let calls = fixture.interaction.calls();
    assert!(calls.iter().any(|(method, params)| {
        method == "process/list" && params["sessionId"] == "telegram:42"
    }));
    assert!(calls.iter().any(|(method, params)| {
        method == "process/read"
            && params["sessionId"] == "telegram:42"
            && params["jobId"] == "job-1"
            && params["maxBytes"] == process_output_read_max_bytes()
    }));
}

#[tokio::test]
async fn telegram_process_command_confirms_write_and_terminate() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;
    fixture
        .interaction
        .set_processes(vec![job_snapshot("job-1", JobStatus::Running)]);
    fixture
        .interaction
        .set_process_output(process_output("job-1", "hello"));

    fixture
        .handler
        .handle_command(telegram_command_with_args(
            16,
            "process",
            "job-1 write input",
        ))
        .await
        .unwrap();
    let write_data = fixture.api.last_sent_callback_data(0, 0);
    fixture
        .handle_callback_with_data("process-write-cb", 621, &write_data)
        .await
        .unwrap();

    fixture
        .handler
        .handle_command(telegram_command_with_args(17, "process", "job-1"))
        .await
        .unwrap();
    let terminate_data = fixture.api.last_sent_callback_data(1, 0);
    fixture
        .handle_callback_with_data("process-terminate-request-cb", 621, &terminate_data)
        .await
        .unwrap();
    let confirm_data = fixture.api.edited_callback_data(0, 0);
    fixture
        .handle_callback_with_data("process-terminate-cb", 621, &confirm_data)
        .await
        .unwrap();

    let calls = fixture.interaction.calls();
    assert!(calls.iter().any(|(method, params)| {
        method == "process/write"
            && params["sessionId"] == "telegram:42"
            && params["jobId"] == "job-1"
            && params["text"] == "input"
    }));
    assert!(calls.iter().any(|(method, params)| {
        method == "process/terminate"
            && params["sessionId"] == "telegram:42"
            && params["jobId"] == "job-1"
    }));
}

#[tokio::test]
async fn telegram_process_command_sends_long_output_as_document() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;
    fixture.interaction.set_process_output(process_output(
        "job-long",
        &"x".repeat(PROCESS_OUTPUT_INLINE_CHAR_LIMIT + 1),
    ));

    fixture
        .handler
        .handle_command(telegram_command_with_args(18, "process", "job-long"))
        .await
        .unwrap();

    assert_eq!(fixture.api.document_requests().len(), 1);
    assert!(fixture.interaction.calls().iter().any(|(method, params)| {
        method == "process/read"
            && params["sessionId"] == "telegram:42"
            && params["jobId"] == "job-long"
    }));
}

#[tokio::test]
async fn telegram_manifest_command_approves_and_applies_proposal() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;
    fixture
        .interaction
        .set_manifest_proposals(vec![manifest_proposal("manifest-proposal-1")]);

    fixture
        .handler
        .handle_command(telegram_command(19, "manifest"))
        .await
        .unwrap();
    let approve_data = fixture.api.last_sent_callback_data(0, 0);
    fixture
        .handle_callback_with_data("manifest-approve-cb", 621, &approve_data)
        .await
        .unwrap();
    let apply_data = fixture.api.edited_callback_data(0, 0);
    fixture
        .handle_callback_with_data("manifest-apply-request-cb", 621, &apply_data)
        .await
        .unwrap();
    let confirm_data = fixture.api.edited_callback_data(0, 0);
    fixture
        .handle_callback_with_data("manifest-apply-cb", 621, &confirm_data)
        .await
        .unwrap();

    let calls = fixture.interaction.calls();
    assert!(calls.iter().any(|(method, params)| {
        method == "manifest/get" && params["sessionId"] == "telegram:42"
    }));
    assert!(calls.iter().any(|(method, params)| {
        method == "manifest/system_prompt/get" && params["sessionId"] == "telegram:42"
    }));
    assert!(calls.iter().any(|(method, params)| {
        method == "manifest/proposals/list" && params["sessionId"] == "telegram:42"
    }));
    assert!(calls.iter().any(|(method, params)| {
        method == "manifest/proposals/approve"
            && params["sessionId"] == "telegram:42"
            && params["proposalId"] == "manifest-proposal-1"
    }));
    assert!(calls.iter().any(|(method, params)| {
        method == "manifest/apply_approved" && params["sessionId"] == "telegram:42"
    }));
}

#[tokio::test]
async fn telegram_subagent_command_spawns_child_and_prompts() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture.establish_session().await;

    fixture
        .handler
        .handle_command(telegram_command_with_args(
            20,
            "subagent",
            "researcher inspect storage",
        ))
        .await
        .unwrap();
    fixture
        .handler
        .handle_command(telegram_command(21, "sessions"))
        .await
        .unwrap();

    let calls = fixture.interaction.calls();
    assert!(calls.iter().any(|(method, params)| {
        method == "subagent/spawn"
            && params["parentSessionId"] == "telegram:42"
            && params["role"] == "researcher"
            && params["metadata"]["channel"] == "telegram"
    }));
    assert!(calls.iter().any(|(method, params)| {
        method == "agent/prompt"
            && params["sessionId"] == "session-subagent-1"
            && params["input"]["message"]["content"][0]["text"] == "inspect storage"
    }));
    assert!(
        fixture
            .api
            .sent_texts()
            .iter()
            .any(|text| text.contains("session\\-subagent\\-1"))
    );
}

#[tokio::test]
async fn telegram_submission_setup_failure_is_reported_without_polling_failure() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture
        .interaction
        .fail_method("session/create", "session store rejected create");

    fixture
        .handler
        .handle_text_input_message(telegram_text_input("hello"))
        .await
        .unwrap();

    assert!(fixture.api.sent_texts().iter().any(|text| {
        text.contains("Message could not be submitted to the agent")
            && text.contains("session store rejected create")
    }));
}

#[tokio::test]
async fn telegram_prompt_jsonrpc_failure_is_left_to_display_delivery() {
    let fixture = TelegramCallbackFixture::new().await;
    fixture
        .interaction
        .fail_method("agent/prompt", "provider rejected media");

    fixture
        .handler
        .handle_text_input_message(telegram_text_input("hello"))
        .await
        .unwrap();

    assert!(
        !fixture
            .api
            .sent_texts()
            .iter()
            .any(|text| text.contains("Message could not be submitted to the agent"))
    );
}

struct TelegramCallbackFixture {
    handler: BridgeUpdateHandler,
    api: Arc<FakeTelegramApi>,
    interaction: Arc<FakeInteraction>,
    callback_data: String,
}

impl TelegramCallbackFixture {
    async fn new() -> Self {
        let api = Arc::new(FakeTelegramApi::default());
        let interaction = Arc::new(FakeInteraction::default());
        let bridge = Arc::new(
            noloong_agent_telegram::bridge::TelegramBridge::new(
                telegram_test_config(),
                interaction.clone(),
            )
            .unwrap(),
        );
        let delivery = TelegramDelivery::new(api.clone(), 3900);
        let display_states = Arc::new(Mutex::new(BTreeMap::new()));
        let key = noloong_agent_telegram::session::TelegramSessionKey::new(42, None);
        let state = Arc::new(Mutex::new(TelegramDisplayState::default()));
        display_states.lock().await.insert(key, state.clone());
        {
            let mut state = state.lock().await;
            deliver_display_event(
                &mut state,
                &delivery,
                TelegramMessageTarget::chat(42),
                approval_notification(),
                true,
                std::time::Duration::ZERO,
                TelegramUiCatalog::new(Locale::En),
            )
            .await
            .unwrap();
        }
        let callback_data = api.sent_callback_data(0, 0);
        let handler = BridgeUpdateHandler {
            bridge,
            api: api.clone(),
            delivery,
            media_resolver: TelegramAttachmentResolver::new(
                api.clone(),
                TelegramFilePolicy::default(),
            ),
            display_states,
            session_actions: Arc::new(Mutex::new(TelegramSessionActionStore::default())),
            catalog: TelegramUiCatalog::new(Locale::En),
            bot_username: None,
        };

        Self {
            handler,
            api,
            interaction,
            callback_data,
        }
    }

    async fn handle_callback(
        &self,
        id: &str,
        user_id: u64,
    ) -> Result<(), noloong_agent_telegram::polling::TelegramPollingError> {
        self.handle_callback_with_data(id, user_id, &self.callback_data)
            .await
    }

    async fn handle_callback_with_data(
        &self,
        id: &str,
        user_id: u64,
        data: &str,
    ) -> Result<(), noloong_agent_telegram::polling::TelegramPollingError> {
        self.handler
            .handle_callback(callback_query(id, user_id, data))
            .await
    }

    async fn establish_session(&self) {
        self.handler
            .bridge
            .handle_text_message(telegram_text_input("hello"), None)
            .await
            .unwrap();
    }
}

fn telegram_test_config() -> noloong_agent_telegram::config::TelegramBridgeConfig {
    telegram_config_from_values(
        &TelegramBridgeOptions {
            interaction_url: Some("ws://127.0.0.1:8787/jsonrpc/ws".into()),
            bot_token: Some("token".into()),
            allowed_users: Some("621".into()),
            profile_id: Some("profile-1".into()),
            ..Default::default()
        },
        |_| None,
    )
    .unwrap()
}

fn approval_notification() -> noloong_agent_telegram::bridge::InteractionDisplayNotification {
    noloong_agent_telegram::bridge::InteractionDisplayNotification {
        session_id: "session-1".into(),
        subscription_id: "subscription-1".into(),
        event: noloong_agent::interaction::DisplayEvent::ApprovalRequested {
            approval: approval_request("approval-1"),
        },
    }
}

fn approval_request(approval_id: &str) -> ToolApprovalRequest {
    ToolApprovalRequest {
        approval_id: approval_id.into(),
        tool_call: ToolCall {
            id: "tool-1".into(),
            name: "host_exec".into(),
            arguments: json!({"cmd": "ls"}),
        },
        permissions: Vec::new(),
        hook_id: None,
        request: ToolApprovalRequestSpec {
            prompt: Some("Run command?".into()),
            reason: None,
            expires_at_ms: None,
            metadata: Value::Object(Default::default()),
        },
    }
}

fn approvals_command() -> TelegramCommand {
    telegram_command(3, "approvals")
}

fn unknown_command() -> TelegramCommand {
    telegram_command(4, "unknown")
}

fn telegram_command(message_id: i64, name: &str) -> TelegramCommand {
    TelegramCommand {
        context: telegram_inbound_context(message_id),
        name: name.into(),
        bot_username: None,
        args: String::new(),
        raw_text: format!("/{name}"),
    }
}

fn telegram_command_with_args(message_id: i64, name: &str, args: &str) -> TelegramCommand {
    TelegramCommand {
        context: telegram_inbound_context(message_id),
        name: name.into(),
        bot_username: None,
        args: args.into(),
        raw_text: format!("/{name} {args}"),
    }
}

fn queued_user_message(id: &str, text: &str) -> TelegramQueuedMessage {
    TelegramQueuedMessage {
        message: AgentMessage::user(id, text),
        intent: TelegramQueuedMessageIntent::UserInput,
    }
}

fn job_snapshot(job_id: &str, status: JobStatus) -> JobSnapshot {
    JobSnapshot {
        job_id: job_id.into(),
        command: "echo hello".into(),
        shell: "sh".into(),
        cwd: PathBuf::from("/tmp"),
        status,
        started_at_ms: 1,
        ended_at_ms: None,
        next_cursor: 1,
        dropped_before_seq: 0,
    }
}

fn process_output(job_id: &str, text: &str) -> ProcessOutput {
    ProcessOutput {
        job_id: job_id.into(),
        chunks: vec![OutputChunk {
            seq: 1,
            stream: ProcessOutputStream::Stdout,
            text: text.into(),
            byte_len: text.len(),
        }],
        next_cursor: 2,
        dropped_before_seq: 0,
        truncated: false,
        status: JobStatus::Running,
    }
}

fn manifest_proposal(proposal_id: &str) -> ManifestPatchProposal {
    ManifestPatchProposal {
        proposal_id: proposal_id.into(),
        patch: ManifestPatch::UpsertSystemPromptAddition {
            addition: SystemPromptAddition::new("telegram.test", "Test addition."),
        },
        summary: "upsert system prompt addition telegram.test".into(),
    }
}

fn telegram_text_input(text: &str) -> TelegramTextInput {
    TelegramTextInput {
        chat_id: 42,
        thread_id: None,
        chat_kind: TelegramChatKind::Private,
        user_id: Some(621),
        message_id: 2,
        text: text.into(),
        is_reply_to_bot: false,
        reply_to: None,
    }
}

fn telegram_inbound_context(message_id: i64) -> TelegramInboundContext {
    TelegramInboundContext {
        chat_id: 42,
        thread_id: None,
        chat_kind: TelegramChatKind::Private,
        user_id: Some(621),
        message_id,
        is_reply_to_bot: false,
        reply_to: None,
    }
}

fn callback_query(id: &str, user_id: u64, data: &str) -> TelegramCallbackQuery {
    TelegramCallbackQuery {
        id: id.into(),
        from: TelegramUser {
            id: user_id,
            username: Some("alice".into()),
        },
        message: Some(TelegramMessage {
            message_id: 10,
            message_thread_id: None,
            chat: TelegramChat {
                id: 42,
                kind: "private".into(),
            },
            from: None,
            text: None,
            caption: None,
            entities: Vec::new(),
            caption_entities: Vec::new(),
            photo: Vec::new(),
            document: None,
            audio: None,
            voice: None,
            video: None,
            reply_to_message: None,
        }),
        data: Some(data.into()),
    }
}

#[derive(Default)]
struct FakeInteraction {
    calls: StdMutex<Vec<(String, Value)>>,
    approval_list: StdMutex<BTreeMap<String, ToolApprovalRequest>>,
    profiles: StdMutex<Vec<InteractionProfileDescriptor>>,
    sessions: StdMutex<Vec<InteractionSessionDescriptor>>,
    queues: StdMutex<BTreeMap<(String, TelegramQueueKind), Vec<TelegramQueuedMessage>>>,
    queue_modes: StdMutex<BTreeMap<(String, TelegramQueueKind), QueueMode>>,
    processes: StdMutex<Vec<JobSnapshot>>,
    process_outputs: StdMutex<BTreeMap<String, ProcessOutput>>,
    manifest_proposals: StdMutex<Vec<ManifestPatchProposal>>,
    approved_manifest_proposals: StdMutex<Vec<ManifestPatchProposal>>,
    failures: StdMutex<BTreeMap<String, String>>,
}

impl FakeInteraction {
    fn methods(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(method, _)| method.clone())
            .collect()
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }

    fn set_approval_list(&self, approvals: BTreeMap<String, ToolApprovalRequest>) {
        *self.approval_list.lock().unwrap() = approvals;
    }

    fn set_profiles(&self, profiles: Vec<InteractionProfileDescriptor>) {
        *self.profiles.lock().unwrap() = profiles;
    }

    fn set_sessions(&self, sessions: Vec<InteractionSessionDescriptor>) {
        *self.sessions.lock().unwrap() = sessions;
    }

    fn set_session_status(&self, session_id: &str, status: InteractionSessionStatus) {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions
            .iter_mut()
            .find(|session| session.session_id == session_id)
        else {
            return;
        };
        session.status = status;
    }

    fn set_queue(
        &self,
        session_id: &str,
        queue: TelegramQueueKind,
        messages: Vec<TelegramQueuedMessage>,
    ) {
        self.queues
            .lock()
            .unwrap()
            .insert((session_id.into(), queue), messages);
    }

    fn set_processes(&self, processes: Vec<JobSnapshot>) {
        *self.processes.lock().unwrap() = processes;
    }

    fn set_process_output(&self, output: ProcessOutput) {
        self.process_outputs
            .lock()
            .unwrap()
            .insert(output.job_id.clone(), output);
    }

    fn set_manifest_proposals(&self, proposals: Vec<ManifestPatchProposal>) {
        *self.manifest_proposals.lock().unwrap() = proposals;
    }

    fn fail_method(&self, method: &str, message: &str) {
        self.failures
            .lock()
            .unwrap()
            .insert(method.into(), message.into());
    }
}

impl TelegramInteractionClient for FakeInteraction {
    fn request_value<'a>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> TelegramInteractionFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((method.into(), params.clone()));
            if let Some(message) = self.failures.lock().unwrap().get(method).cloned() {
                return Err(TelegramBridgeError::Interaction(
                    InteractionClientError::JsonRpc {
                        code: -32603,
                        message,
                        data: None,
                    },
                ));
            }
            match method {
                "approval/list" => {
                    let approvals = self.approval_list.lock().unwrap().clone();
                    Ok(serde_json::to_value(approvals).unwrap())
                }
                "profile/list" => {
                    let profiles = self.profiles.lock().unwrap().clone();
                    let profiles = if profiles.is_empty() {
                        vec![profile_descriptor("profile-1")]
                    } else {
                        profiles
                    };
                    Ok(serde_json::to_value(profiles).unwrap())
                }
                "session/create" => {
                    let session_id = params["sessionId"]
                        .as_str()
                        .unwrap_or("session-1")
                        .to_owned();
                    let profile_id = params["profileId"]
                        .as_str()
                        .unwrap_or("profile-1")
                        .to_owned();
                    let descriptor = session_descriptor_with(
                        &session_id,
                        &profile_id,
                        InteractionSessionStatus::Idle,
                        params["metadata"].as_object().cloned().unwrap_or_default(),
                    );
                    self.sessions.lock().unwrap().push(descriptor.clone());
                    Ok(serde_json::to_value(descriptor).unwrap())
                }
                "session/list" => {
                    let sessions = self.sessions.lock().unwrap().clone();
                    Ok(serde_json::to_value(sessions).unwrap())
                }
                "session/get" => {
                    let session_id = params["sessionId"].as_str().unwrap_or("session-1");
                    let descriptor = self
                        .sessions
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|session| session.session_id == session_id)
                        .cloned()
                        .unwrap_or_else(|| {
                            session_descriptor_with(
                                session_id,
                                "profile-1",
                                InteractionSessionStatus::Idle,
                                Default::default(),
                            )
                        });
                    Ok(serde_json::to_value(descriptor).unwrap())
                }
                "session/delete" => {
                    let session_id = params["sessionId"].as_str().unwrap_or("session-1");
                    let deleted = {
                        let mut sessions = self.sessions.lock().unwrap();
                        let index = sessions
                            .iter()
                            .position(|session| session.session_id == session_id);
                        index.map(|index| sessions.remove(index))
                    };
                    Ok(serde_json::to_value(deleted).unwrap())
                }
                "agent/continue" => {
                    let request = parse_fake_request::<FakeSessionRequest>(params);
                    Ok(serde_json::to_value(self.session_by_id(&request.session_id)).unwrap())
                }
                "agent/abort" => {
                    let request = parse_fake_request::<FakeSessionRequest>(params);
                    let mut descriptor = self.session_by_id(&request.session_id);
                    descriptor.status = InteractionSessionStatus::Aborted;
                    Ok(serde_json::to_value(descriptor).unwrap())
                }
                "agent/follow_up" => {
                    let request = parse_fake_request::<FakeFollowUpRequest>(params);
                    self.queues
                        .lock()
                        .unwrap()
                        .entry((request.session_id.clone(), TelegramQueueKind::FollowUp))
                        .or_default()
                        .push(TelegramQueuedMessage {
                            message: request.message,
                            intent: TelegramQueuedMessageIntent::UserInput,
                        });
                    Ok(serde_json::to_value(self.session_by_id(&request.session_id)).unwrap())
                }
                "agent/prompt" => {
                    let request = parse_fake_request::<FakePromptRequest>(params);
                    let mut descriptor = self.session_by_id(&request.session_id);
                    let _input = request.input;
                    descriptor.status = InteractionSessionStatus::Running;
                    self.upsert_session(descriptor.clone());
                    Ok(serde_json::to_value(descriptor).unwrap())
                }
                "queue/list" => {
                    let request = parse_fake_request::<FakeQueueRequest>(params);
                    let messages = self.queue_messages(&request.session_id, request.queue);
                    Ok(serde_json::to_value(messages).unwrap())
                }
                "queue/clear" => {
                    let request = parse_fake_request::<FakeQueueRequest>(params);
                    self.queues
                        .lock()
                        .unwrap()
                        .insert((request.session_id, request.queue), Vec::new());
                    Ok(serde_json::to_value(Vec::<TelegramQueuedMessage>::new()).unwrap())
                }
                "queue/set_mode" => {
                    let request = parse_fake_request::<FakeQueueSetModeRequest>(params);
                    let FakeQueueSetModeRequest {
                        session_id,
                        queue,
                        mode,
                    } = request;
                    self.queue_modes
                        .lock()
                        .unwrap()
                        .insert((session_id.clone(), queue), mode);
                    let messages = self.queue_messages(&session_id, queue);
                    Ok(serde_json::to_value(messages).unwrap())
                }
                "process/list" => {
                    let _request = parse_fake_request::<FakeSessionRequest>(params);
                    Ok(serde_json::to_value(self.processes.lock().unwrap().clone()).unwrap())
                }
                "process/read" => {
                    let request = parse_fake_request::<FakeProcessReadRequest>(params);
                    let FakeProcessReadRequest {
                        session_id: _session_id,
                        job_id,
                        after_seq: _after_seq,
                        max_bytes: _max_bytes,
                        wait_ms: _wait_ms,
                    } = request;
                    Ok(serde_json::to_value(self.process_output(&job_id)).unwrap())
                }
                "process/wait" => {
                    let request = parse_fake_request::<FakeProcessWaitRequest>(params);
                    let FakeProcessWaitRequest {
                        session_id: _session_id,
                        job_id,
                        timeout_ms: _timeout_ms,
                    } = request;
                    let output = self.process_output(&job_id);
                    Ok(serde_json::to_value(WaitOutcome {
                        job_id,
                        status: output.status,
                        timed_out: false,
                    })
                    .unwrap())
                }
                "process/write" => {
                    let request = parse_fake_request::<FakeProcessWriteRequest>(params);
                    let FakeProcessWriteRequest {
                        session_id: _session_id,
                        job_id,
                        text: _text,
                    } = request;
                    Ok(
                        serde_json::to_value(self.process_snapshot(&job_id, JobStatus::Running))
                            .unwrap(),
                    )
                }
                "process/terminate" => {
                    let request = parse_fake_request::<FakeProcessJobRequest>(params);
                    let FakeProcessJobRequest {
                        session_id: _session_id,
                        job_id,
                    } = request;
                    Ok(
                        serde_json::to_value(self.process_snapshot(&job_id, JobStatus::Terminated))
                            .unwrap(),
                    )
                }
                "manifest/get" => {
                    let request = parse_fake_request::<FakeSessionRequest>(params);
                    Ok(
                        serde_json::to_value(self.session_by_id(&request.session_id).manifest)
                            .unwrap(),
                    )
                }
                "manifest/system_prompt/get" => {
                    let request = parse_fake_request::<FakeSessionRequest>(params);
                    let manifest = self.session_by_id(&request.session_id).manifest;
                    let prompt = noloong_agent::system_prompt::resolve_system_prompt(
                        manifest.locale,
                        &manifest.system_prompt,
                        None,
                    );
                    Ok(serde_json::to_value(prompt).unwrap())
                }
                "manifest/proposals/list" => Ok(serde_json::to_value(
                    self.manifest_proposals.lock().unwrap().clone(),
                )
                .unwrap()),
                "manifest/proposals/approve" => {
                    let request = parse_fake_request::<FakeManifestProposalRequest>(params);
                    let FakeManifestProposalRequest {
                        session_id: _session_id,
                        proposal_id,
                    } = request;
                    let proposal = {
                        let mut proposals = self.manifest_proposals.lock().unwrap();
                        let index = proposals
                            .iter()
                            .position(|proposal| proposal.proposal_id == proposal_id)
                            .unwrap();
                        proposals.remove(index)
                    };
                    self.approved_manifest_proposals
                        .lock()
                        .unwrap()
                        .push(proposal.clone());
                    Ok(serde_json::to_value(proposal).unwrap())
                }
                "manifest/apply_approved" => {
                    let _request = parse_fake_request::<FakeSessionRequest>(params);
                    let applied_proposal_ids = self
                        .approved_manifest_proposals
                        .lock()
                        .unwrap()
                        .drain(..)
                        .map(|proposal| proposal.proposal_id)
                        .collect::<Vec<_>>();
                    Ok(serde_json::json!({
                        "appliedProposalIds": applied_proposal_ids
                    }))
                }
                "subagent/spawn" => {
                    let request = parse_fake_request::<FakeSubagentSpawnRequest>(params);
                    let descriptor = session_descriptor_with_parent(
                        "session-subagent-1",
                        "profile-1",
                        Some(request.parent_session_id),
                        request.role,
                        InteractionSessionStatus::Idle,
                        request.metadata,
                    );
                    self.upsert_session(descriptor.clone());
                    Ok(serde_json::to_value(descriptor).unwrap())
                }
                "display/subscribe" => Ok(json!({"subscriptionId": "subscription-1"})),
                _ => Ok(serde_json::to_value(session_descriptor()).unwrap()),
            }
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<InteractionWsNotification> {
        let (_sender, receiver) = broadcast::channel(1);
        receiver
    }
}

impl FakeInteraction {
    fn session_by_id(&self, session_id: &str) -> InteractionSessionDescriptor {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .find(|session| session.session_id == session_id)
            .cloned()
            .unwrap_or_else(|| {
                session_descriptor_with(
                    session_id,
                    "profile-1",
                    InteractionSessionStatus::Idle,
                    Default::default(),
                )
            })
    }

    fn queue_messages(
        &self,
        session_id: &str,
        queue: TelegramQueueKind,
    ) -> Vec<TelegramQueuedMessage> {
        self.queues
            .lock()
            .unwrap()
            .get(&(session_id.into(), queue))
            .cloned()
            .unwrap_or_default()
    }

    fn process_output(&self, job_id: &str) -> ProcessOutput {
        self.process_outputs
            .lock()
            .unwrap()
            .get(job_id)
            .cloned()
            .unwrap_or_else(|| process_output(job_id, ""))
    }

    fn process_snapshot(&self, job_id: &str, status: JobStatus) -> JobSnapshot {
        self.processes
            .lock()
            .unwrap()
            .iter()
            .find(|snapshot| snapshot.job_id == job_id)
            .cloned()
            .map(|mut snapshot| {
                snapshot.status = status.clone();
                snapshot
            })
            .unwrap_or_else(|| job_snapshot(job_id, status))
    }

    fn upsert_session(&self, descriptor: InteractionSessionDescriptor) {
        let mut sessions = self.sessions.lock().unwrap();
        match sessions
            .iter_mut()
            .find(|session| session.session_id == descriptor.session_id)
        {
            Some(session) => *session = descriptor,
            None => sessions.push(descriptor),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeSessionRequest {
    session_id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeFollowUpRequest {
    session_id: String,
    message: AgentMessage,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakePromptRequest {
    session_id: String,
    input: Value,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeQueueRequest {
    session_id: String,
    queue: TelegramQueueKind,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeQueueSetModeRequest {
    session_id: String,
    queue: TelegramQueueKind,
    mode: QueueMode,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeProcessJobRequest {
    session_id: String,
    job_id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeProcessReadRequest {
    session_id: String,
    job_id: String,
    after_seq: Option<u64>,
    max_bytes: Option<usize>,
    wait_ms: Option<u64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeProcessWaitRequest {
    session_id: String,
    job_id: String,
    timeout_ms: Option<u64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeProcessWriteRequest {
    session_id: String,
    job_id: String,
    text: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeManifestProposalRequest {
    session_id: String,
    proposal_id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FakeSubagentSpawnRequest {
    parent_session_id: String,
    role: Option<String>,
    metadata: serde_json::Map<String, Value>,
}

fn parse_fake_request<T>(params: Value) -> T
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(params).unwrap()
}

fn profile_descriptor(profile_id: &str) -> InteractionProfileDescriptor {
    InteractionProfileDescriptor {
        profile_id: profile_id.into(),
        display_name: profile_id.into(),
        description: None,
        default_manifest_patches: Vec::new(),
        metadata: Default::default(),
    }
}

fn session_descriptor() -> InteractionSessionDescriptor {
    session_descriptor_with(
        "session-1",
        "profile-1",
        InteractionSessionStatus::Idle,
        Default::default(),
    )
}

fn telegram_session_descriptor(session_id: &str) -> InteractionSessionDescriptor {
    telegram_session_descriptor_with_status(session_id, InteractionSessionStatus::Idle)
}

fn telegram_session_descriptor_with_status(
    session_id: &str,
    status: InteractionSessionStatus,
) -> InteractionSessionDescriptor {
    session_descriptor_with(
        session_id,
        "profile-1",
        status,
        telegram_session_metadata(42, None, "private"),
    )
}

fn session_descriptor_with(
    session_id: &str,
    profile_id: &str,
    status: InteractionSessionStatus,
    metadata: serde_json::Map<String, Value>,
) -> InteractionSessionDescriptor {
    session_descriptor_with_parent(session_id, profile_id, None, None, status, metadata)
}

fn session_descriptor_with_parent(
    session_id: &str,
    profile_id: &str,
    parent_session_id: Option<String>,
    role: Option<String>,
    status: InteractionSessionStatus,
    metadata: serde_json::Map<String, Value>,
) -> InteractionSessionDescriptor {
    InteractionSessionDescriptor {
        session_id: session_id.into(),
        profile_id: profile_id.into(),
        parent_session_id,
        role,
        status,
        manifest: AgentManifest::default(),
        state: AgentState::default(),
        metadata,
    }
}

#[derive(Default)]
struct FakeTelegramApi {
    sent: StdMutex<Vec<TelegramSendMessageRequest>>,
    edited: StdMutex<Vec<TelegramEditMessageTextRequest>>,
    deleted: StdMutex<Vec<TelegramDeleteMessageRequest>>,
    documents: StdMutex<Vec<TelegramSendDocumentRequest>>,
    answered: StdMutex<Vec<(String, Option<String>)>>,
    command_requests: StdMutex<Vec<TelegramSetMyCommandsRequest>>,
}

impl FakeTelegramApi {
    fn sent_callback_data(&self, row: usize, column: usize) -> String {
        self.sent.lock().unwrap()[0]
            .reply_markup
            .as_ref()
            .unwrap()
            .inline_keyboard[row][column]
            .callback_data
            .clone()
    }

    fn last_sent_callback_data(&self, row: usize, column: usize) -> String {
        self.sent
            .lock()
            .unwrap()
            .last()
            .and_then(|request| request.reply_markup.as_ref())
            .unwrap()
            .inline_keyboard[row][column]
            .callback_data
            .clone()
    }

    fn sent_texts(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.text.clone())
            .collect()
    }

    fn answered_texts(&self) -> Vec<Option<String>> {
        self.answered
            .lock()
            .unwrap()
            .iter()
            .map(|(_, text)| text.clone())
            .collect()
    }

    fn edited_reply_markup(
        &self,
    ) -> Vec<Option<noloong_agent_telegram::telegram_api::TelegramInlineKeyboardMarkup>> {
        self.edited
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.reply_markup.clone())
            .collect()
    }

    fn edited_texts(&self) -> Vec<String> {
        self.edited
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.text.clone())
            .collect()
    }

    fn edited_callback_data(&self, row: usize, column: usize) -> String {
        self.edited
            .lock()
            .unwrap()
            .last()
            .and_then(|request| request.reply_markup.as_ref())
            .unwrap()
            .inline_keyboard[row][column]
            .callback_data
            .clone()
    }

    fn deleted_message_ids(&self) -> Vec<i64> {
        self.deleted
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.message_id)
            .collect()
    }

    fn command_requests(&self) -> Vec<TelegramSetMyCommandsRequest> {
        self.command_requests.lock().unwrap().clone()
    }

    fn document_requests(&self) -> Vec<TelegramSendDocumentRequest> {
        self.documents.lock().unwrap().clone()
    }
}

impl TelegramApi for FakeTelegramApi {
    fn get_updates<'a>(
        &'a self,
        _offset: Option<i64>,
        _timeout_seconds: u64,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<TelegramUpdate>, TelegramApiError>> + Send + 'a>>
    {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn send_message<'a>(
        &'a self,
        request: TelegramSendMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.sent.lock().unwrap().push(request.clone());
            Ok(TelegramMessageHandle {
                chat_id: request.chat_id,
                message_id: 10,
            })
        })
    }

    fn edit_message_text<'a>(
        &'a self,
        request: TelegramEditMessageTextRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.edited.lock().unwrap().push(request.clone());
            Ok(TelegramMessageHandle {
                chat_id: request.chat_id,
                message_id: request.message_id,
            })
        })
    }

    fn delete_message<'a>(
        &'a self,
        request: TelegramDeleteMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<(), TelegramApiError>> + Send + 'a>> {
        Box::pin(async move {
            self.deleted.lock().unwrap().push(request);
            Ok(())
        })
    }

    fn send_document<'a>(
        &'a self,
        request: TelegramSendDocumentRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TelegramMessageHandle, TelegramApiError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.documents.lock().unwrap().push(request.clone());
            Ok(TelegramMessageHandle {
                chat_id: request.chat_id,
                message_id: 11,
            })
        })
    }

    fn answer_callback_query<'a>(
        &'a self,
        callback_query_id: &'a str,
        text: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<(), TelegramApiError>> + Send + 'a>> {
        Box::pin(async move {
            self.answered
                .lock()
                .unwrap()
                .push((callback_query_id.into(), text.map(str::to_owned)));
            Ok(())
        })
    }

    fn set_my_commands<'a>(
        &'a self,
        request: TelegramSetMyCommandsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<(), TelegramApiError>> + Send + 'a>> {
        Box::pin(async move {
            self.command_requests.lock().unwrap().push(request);
            Ok(())
        })
    }
}

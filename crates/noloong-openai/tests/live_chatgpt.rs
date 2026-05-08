use noloong_agent_core::{
    AgentCoreError, AgentMessage, AgentRuntime, CancellationToken, ContentBlock,
    ContextCompactionOutput, ContextCompactionRequest, ContextCompactor, MessageRole,
    Result as CoreResult,
};
use noloong_openai::{
    auth::{
        ChatGptAuthManager, ChatGptEphemeralTokenStorage, ChatGptTokenData, ChatGptTokenStorage,
        ChatGptTokenStore,
    },
    compact::{OpenAiResponsesCompactor, OpenAiResponsesCompactorConfig},
    provider::chatgpt_responses_provider,
};
use serde_json::Map;
use std::{env, path::PathBuf, sync::Arc};

#[path = "support/logging.rs"]
mod logging;
use logging::init_test_logger;

const LIVE_ENABLE_ENV: &str = "NOLOONG_OPENAI_LIVE_CHATGPT";
const LIVE_MODEL_ENV: &str = "NOLOONG_CHATGPT_LIVE_MODEL";
const LIVE_TOKEN_FILE_ENV: &str = "NOLOONG_CHATGPT_TOKEN_FILE";
const LIVE_ID_TOKEN_ENV: &str = "NOLOONG_CHATGPT_ID_TOKEN";
const LIVE_ACCESS_TOKEN_ENV: &str = "NOLOONG_CHATGPT_ACCESS_TOKEN";
const LIVE_REFRESH_TOKEN_ENV: &str = "NOLOONG_CHATGPT_REFRESH_TOKEN";
const LIVE_ACCOUNT_ID_ENV: &str = "NOLOONG_CHATGPT_ACCOUNT_ID";

#[tokio::test]
#[ignore = "requires ChatGPT subscription auth and external ChatGPT Codex backend access"]
async fn live_chatgpt_responses_streaming_smoke() -> CoreResult<()> {
    let Some((model, auth)) = live_model_and_auth()? else {
        return Ok(());
    };
    let sentinel = "noloong-chatgpt-live-ok";
    let runtime = AgentRuntime::builder()
        .with_model_provider(Arc::new(chatgpt_responses_provider(
            "chatgpt-live",
            model,
            auth,
        )?))
        .max_turns(1)
        .build()?;

    let report = runtime
        .run(format!(
            "Reply with exactly `{sentinel}` as the only visible text."
        ))
        .await?;

    let visible_text = report
        .state
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        visible_text.contains(sentinel),
        "ChatGPT live response did not contain expected sentinel"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires ChatGPT subscription auth and external ChatGPT compact endpoint access"]
async fn live_chatgpt_responses_compact_smoke() -> CoreResult<()> {
    let Some((model, auth)) = live_model_and_auth()? else {
        return Ok(());
    };
    let compactor = OpenAiResponsesCompactor::new(
        OpenAiResponsesCompactorConfig::new("chatgpt-live-compact", model).auth_provider(auth),
    )?;

    let output = compactor
        .compact(sample_compaction_request(), CancellationToken::new())
        .await?;
    assert!(
        matches!(output, ContextCompactionOutput::Replacement(_)),
        "ChatGPT compact endpoint did not return replacement history"
    );
    Ok(())
}

fn live_model_and_auth() -> CoreResult<Option<(String, Arc<ChatGptAuthManager>)>> {
    if env::var(LIVE_ENABLE_ENV).as_deref() != Ok("1") {
        init_test_logger();
        log::info!("skipping ChatGPT live test; set {LIVE_ENABLE_ENV}=1");
        return Ok(None);
    }
    let Ok(model) = env::var(LIVE_MODEL_ENV) else {
        init_test_logger();
        log::info!("skipping ChatGPT live test; set {LIVE_MODEL_ENV}");
        return Ok(None);
    };
    let Some(storage) = live_token_storage().map_err(to_core_error)? else {
        init_test_logger();
        log::info!(
            "skipping ChatGPT live test; set {LIVE_TOKEN_FILE_ENV} or explicit token env vars"
        );
        return Ok(None);
    };
    Ok(Some((model, Arc::new(ChatGptAuthManager::new(storage)))))
}

fn live_token_storage() -> noloong_openai::Result<Option<Arc<dyn ChatGptTokenStore>>> {
    if let Ok(path) = env::var(LIVE_TOKEN_FILE_ENV) {
        return Ok(Some(Arc::new(ChatGptTokenStorage::file(PathBuf::from(
            path,
        )))));
    }

    let (Ok(id_token), Ok(access_token), Ok(refresh_token)) = (
        env::var(LIVE_ID_TOKEN_ENV),
        env::var(LIVE_ACCESS_TOKEN_ENV),
        env::var(LIVE_REFRESH_TOKEN_ENV),
    ) else {
        return Ok(None);
    };
    let storage = ChatGptEphemeralTokenStorage::new();
    let mut token = ChatGptTokenData::new(id_token, access_token, refresh_token, unix_timestamp()?);
    if let Ok(account_id) = env::var(LIVE_ACCOUNT_ID_ENV) {
        token = token.account_id(account_id);
    }
    storage.save(&token)?;
    Ok(Some(Arc::new(storage)))
}

fn sample_compaction_request() -> ContextCompactionRequest {
    ContextCompactionRequest {
        run_id: "chatgpt-live-compact-run".into(),
        turn_id: 1,
        current_messages: vec![
            AgentMessage {
                id: "system-live".into(),
                role: MessageRole::System,
                content: vec![ContentBlock::Text {
                    text: "Compact this short transcript.".into(),
                }],
                metadata: Map::new(),
            },
            AgentMessage::user("user-live", "Remember that the test sentinel is noloong."),
            AgentMessage::assistant(
                "assistant-live",
                vec![ContentBlock::Text {
                    text: "Noted.".into(),
                }],
            ),
        ],
        previous_summary: None,
        messages_to_summarize: Vec::new(),
        turn_prefix_messages: Vec::new(),
        retained_messages: Vec::new(),
        token_budget: 1_024,
        tokens_before: 2_048,
        estimated_retained_tokens: 0,
        is_split_turn: false,
        metadata: Map::new(),
    }
}

fn unix_timestamp() -> noloong_openai::Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| noloong_openai::OpenAiIntegrationError::Login(error.to_string()))?
        .as_secs())
}

fn to_core_error(error: noloong_openai::OpenAiIntegrationError) -> AgentCoreError {
    AgentCoreError::Provider(error.to_string())
}

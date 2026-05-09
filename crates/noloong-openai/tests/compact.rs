mod support;

use noloong_agent_core::{
    AgentCoreError, AgentMessage, CancellationToken, ContentBlock, ContextCompactionOutput,
    ContextCompactionRequest, ResponsesApiRequestRenderConfig, ResponsesReasoningConfig,
    ResponsesReasoningEffort, ResponsesStateMode, ToolSpec,
};
use noloong_openai::auth::{
    ChatGptAuthManager, ChatGptAuthManagerConfig, ChatGptEphemeralTokenStorage, ChatGptTokenData,
    ChatGptTokenStore,
};
use noloong_openai::compact::{
    OPENAI_RESPONSES_PAYLOAD_PROVIDER, OPENAI_RESPONSES_RESPONSE_ITEM_KIND,
    OpenAiResponsesCompactor, OpenAiResponsesCompactorConfig,
};
use serde_json::{Map, json};
use std::{error::Error, sync::Arc};
use support::{MockHttpServer, MockResponse, unsigned_jwt};

#[tokio::test]
async fn compact_posts_rendered_payload_and_returns_provider_payload_replacement()
-> Result<(), Box<dyn Error>> {
    let output_item = json!({
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "output_text", "text": "compacted" }]
    });
    let server = MockHttpServer::spawn(vec![MockResponse::json(
        200,
        json!({ "output": [output_item.clone()] }),
    )])
    .await?;
    let render = ResponsesApiRequestRenderConfig::new("compact-provider", "model-under-test")
        .reasoning(ResponsesReasoningConfig::new().effort(ResponsesReasoningEffort::Low))
        .text_controls(json!({ "format": { "type": "text" } }));
    let compactor = OpenAiResponsesCompactor::new(
        OpenAiResponsesCompactorConfig::new("compact-provider", "unused")
            .base_url(server.base_url())
            .render(render)
            .tool(ToolSpec {
                name: "lookup".into(),
                description: "Lookup data.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
                execution_mode: None,
                permissions: Vec::new(),
            }),
    )?;

    let output = compactor
        .compact_request(sample_request(), CancellationToken::new())
        .await?;

    let ContextCompactionOutput::Replacement(replacement) = output else {
        panic!("compact endpoint should return replacement output");
    };
    assert_eq!(replacement.replacement_messages.len(), 1);
    let ContentBlock::ProviderPayload {
        provider,
        kind,
        value,
    } = &replacement.replacement_messages[0].content[0]
    else {
        panic!("replacement should preserve raw Responses item");
    };
    assert_eq!(provider, OPENAI_RESPONSES_PAYLOAD_PROVIDER);
    assert_eq!(kind, OPENAI_RESPONSES_RESPONSE_ITEM_KIND);
    assert_eq!(value, &output_item);

    let requests = server.finish().await;
    assert!(requests[0].starts_with("POST /responses/compact "));
    assert!(requests[0].contains(r#""model":"model-under-test""#));
    assert!(requests[0].contains(r#""parallel_tool_calls":true"#));
    assert!(requests[0].contains(r#""reasoning":{"effort":"low"}"#));
    assert!(requests[0].contains(r#""text":{"format":{"type":"text"}}"#));
    assert!(requests[0].contains(r#""instructions":"compact carefully""#));
    assert!(requests[0].contains(r#""name":"lookup""#));
    assert!(!request_body(&requests[0]).contains(r#""stream""#));
    assert!(!request_body(&requests[0]).contains(r#""store""#));
    assert!(!request_body(&requests[0]).contains(r#""include""#));
    Ok(())
}

#[tokio::test]
async fn compact_filters_stateless_unsafe_output_items() -> Result<(), Box<dyn Error>> {
    let encrypted_reasoning = json!({
        "type": "reasoning",
        "id": "rs-1",
        "encrypted_content": "ciphertext"
    });
    let unsafe_reasoning = json!({
        "type": "reasoning",
        "id": "rs-2"
    });
    let server = MockHttpServer::spawn(vec![MockResponse::json(
        200,
        json!({ "output": [unsafe_reasoning, encrypted_reasoning.clone()] }),
    )])
    .await?;
    let compactor = OpenAiResponsesCompactor::new(
        OpenAiResponsesCompactorConfig::new("compact-provider", "model-under-test")
            .base_url(server.base_url()),
    )?;

    let output = compactor
        .compact_request(sample_request(), CancellationToken::new())
        .await?;

    let ContextCompactionOutput::Replacement(replacement) = output else {
        panic!("compact endpoint should return replacement output");
    };
    assert_eq!(replacement.replacement_messages.len(), 1);
    let ContentBlock::ProviderPayload { value, .. } =
        &replacement.replacement_messages[0].content[0]
    else {
        panic!("replacement should preserve raw Responses item");
    };
    assert_eq!(
        value,
        &json!({
            "type": "reasoning",
            "encrypted_content": "ciphertext"
        })
    );
    Ok(())
}

#[tokio::test]
async fn compact_errors_when_stateless_output_has_no_replayable_items() -> Result<(), Box<dyn Error>>
{
    let server = MockHttpServer::spawn(vec![MockResponse::json(
        200,
        json!({ "output": [{ "type": "function_call", "id": "fc-1" }] }),
    )])
    .await?;
    let compactor = OpenAiResponsesCompactor::new(
        OpenAiResponsesCompactorConfig::new("compact-provider", "model-under-test")
            .base_url(server.base_url()),
    )?;

    let error = compactor
        .compact_request(sample_request(), CancellationToken::new())
        .await
        .expect_err("empty replayable compact output should fail");

    assert!(error.to_string().contains("replayable"));
    Ok(())
}

#[tokio::test]
async fn compact_preserves_stateful_output_item_ids() -> Result<(), Box<dyn Error>> {
    let output_item = json!({
        "type": "reasoning",
        "id": "rs-stateful",
    });
    let server = MockHttpServer::spawn(vec![MockResponse::json(
        200,
        json!({ "output": [output_item.clone()] }),
    )])
    .await?;
    let compactor = OpenAiResponsesCompactor::new(
        OpenAiResponsesCompactorConfig::new("compact-provider", "model-under-test")
            .base_url(server.base_url())
            .state_mode(ResponsesStateMode::Stateful),
    )?;

    let output = compactor
        .compact_request(sample_request(), CancellationToken::new())
        .await?;

    let ContextCompactionOutput::Replacement(replacement) = output else {
        panic!("compact endpoint should return replacement output");
    };
    let ContentBlock::ProviderPayload { value, .. } =
        &replacement.replacement_messages[0].content[0]
    else {
        panic!("replacement should preserve raw Responses item");
    };
    assert_eq!(value, &output_item);
    Ok(())
}

#[tokio::test]
async fn compact_refreshes_chatgpt_auth_once_after_unauthorized() -> Result<(), Box<dyn Error>> {
    let server = MockHttpServer::spawn(vec![
        MockResponse::text(401, "unauthorized"),
        MockResponse::json(
            200,
            json!({
                "id_token": id_token("account-123"),
                "access_token": "new-access-token",
                "refresh_token": "new-refresh-token"
            }),
        ),
        MockResponse::json(
            200,
            json!({
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "compacted" }]
                }]
            }),
        ),
    ])
    .await?;
    let auth = Arc::new(auth_manager_for_server(&server)?);
    let compactor = OpenAiResponsesCompactor::new(
        OpenAiResponsesCompactorConfig::new("compact-provider", "model-under-test")
            .base_url(server.base_url())
            .auth_provider(auth),
    )?;

    let output = compactor
        .compact_request(sample_request(), CancellationToken::new())
        .await?;

    assert!(matches!(output, ContextCompactionOutput::Replacement(_)));
    let requests = server.finish().await;
    assert!(requests[0].starts_with("POST /responses/compact "));
    assert!(requests[0].contains("authorization: Bearer old-access-token"));
    assert!(requests[0].contains(r#""tools":[]"#));
    assert!(requests[1].starts_with("POST /oauth/token "));
    assert!(requests[2].starts_with("POST /responses/compact "));
    assert!(requests[2].contains("authorization: Bearer new-access-token"));
    Ok(())
}

#[tokio::test]
async fn compact_returns_http_status_error_without_replacement_on_provider_failure()
-> Result<(), Box<dyn Error>> {
    let server = MockHttpServer::spawn(vec![MockResponse::text(400, "bad request")]).await?;
    let compactor = OpenAiResponsesCompactor::new(
        OpenAiResponsesCompactorConfig::new("compact-provider", "model-under-test")
            .base_url(server.base_url()),
    )?;

    let error = compactor
        .compact_request(sample_request(), CancellationToken::new())
        .await
        .expect_err("provider failure should not produce replacement output");

    assert!(matches!(
        error,
        AgentCoreError::HttpStatus { status: 400, .. }
    ));
    let requests = server.finish().await;
    assert_eq!(requests.len(), 1);
    Ok(())
}

fn sample_request() -> ContextCompactionRequest {
    ContextCompactionRequest {
        run_id: "run-1".into(),
        turn_id: 7,
        current_messages: vec![
            AgentMessage {
                id: "system-1".into(),
                role: noloong_agent_core::MessageRole::System,
                content: vec![ContentBlock::Text {
                    text: "compact carefully".into(),
                }],
                metadata: Map::new(),
            },
            AgentMessage::user("user-1", "hello"),
        ],
        previous_summary: None,
        messages_to_summarize: Vec::new(),
        turn_prefix_messages: Vec::new(),
        retained_messages: Vec::new(),
        token_budget: 1024,
        tokens_before: 2048,
        estimated_retained_tokens: 0,
        is_split_turn: false,
        metadata: Map::new(),
    }
}

fn auth_manager_for_server(server: &MockHttpServer) -> noloong_openai::Result<ChatGptAuthManager> {
    let storage = Arc::new(ChatGptEphemeralTokenStorage::new());
    storage.save(&ChatGptTokenData::new(
        id_token("account-123"),
        "old-access-token",
        "old-refresh-token",
        unix_timestamp()?,
    ))?;
    Ok(ChatGptAuthManager::with_config(
        ChatGptAuthManagerConfig::new()
            .client_id("client-id")
            .refresh_endpoint(format!("{}/oauth/token", server.base_url())),
        storage,
    ))
}

fn id_token(account_id: &str) -> String {
    unsigned_jwt(json!({
        "https://api.openai.com/auth": {
            "chatgpt_account_id": account_id
        }
    }))
}

fn request_body(request: &str) -> &str {
    request.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn unix_timestamp() -> noloong_openai::Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| noloong_openai::OpenAiIntegrationError::Login(error.to_string()))?
        .as_secs())
}

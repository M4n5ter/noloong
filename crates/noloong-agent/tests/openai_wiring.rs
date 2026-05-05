#![cfg(feature = "openai")]

use noloong_agent::AgentSession;
use noloong_agent_core::{
    BoxFuture, CancellationToken, ContextCompactionConfig, HttpAuthContext, HttpAuthHeader,
    HttpAuthHeaders, HttpAuthProvider, HttpAuthRefreshContext, HttpAuthRefreshResult,
};
use noloong_openai::compact::OpenAiResponsesCompactorConfig;
use std::sync::Arc;

#[test]
fn openai_feature_registers_chatgpt_responses_provider_explicitly() -> noloong_agent_core::Result<()>
{
    let runtime = AgentSession::builder()
        .build()
        .runtime_builder()
        .with_chatgpt_responses_provider(
            "chatgpt-responses",
            "model-under-test",
            Arc::new(StaticAuthProvider),
        )?
        .build()?;

    let provider = runtime.default_model_provider()?;
    assert_eq!(provider.id(), "chatgpt-responses");
    assert_eq!(provider.model_name(), Some("model-under-test"));
    Ok(())
}

#[test]
fn openai_feature_registers_compactor_without_changing_default_provider()
-> noloong_agent_core::Result<()> {
    let runtime = AgentSession::builder()
        .build()
        .runtime_builder()
        .with_chatgpt_responses_provider(
            "chatgpt-responses",
            "model-under-test",
            Arc::new(StaticAuthProvider),
        )?
        .with_openai_responses_compactor(
            ContextCompactionConfig::new(1_000)
                .reserve_tokens(100)
                .keep_recent_tokens(10),
            OpenAiResponsesCompactorConfig::new("openai-compact", "model-under-test")
                .base_url("http://127.0.0.1:1"),
        )?
        .build()?;

    assert_eq!(runtime.default_model_provider()?.id(), "chatgpt-responses");
    Ok(())
}

struct StaticAuthProvider;

impl HttpAuthProvider for StaticAuthProvider {
    fn id(&self) -> &str {
        "static-auth"
    }

    fn headers<'a>(
        &'a self,
        _context: HttpAuthContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, HttpAuthHeaders> {
        Box::pin(async {
            Ok(HttpAuthHeaders::new(vec![HttpAuthHeader::new(
                "Authorization",
                "Bearer token",
            )]))
        })
    }

    fn refresh<'a>(
        &'a self,
        _context: HttpAuthRefreshContext,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, HttpAuthRefreshResult> {
        Box::pin(async { Ok(HttpAuthRefreshResult::deny()) })
    }
}

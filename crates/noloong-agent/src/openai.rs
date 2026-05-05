use crate::AgentSessionRuntimeBuilder;
use noloong_agent_core::{
    ContextCompactionConfig, HttpAuthProvider, Result as CoreResult, TokenEstimator,
};
use noloong_openai::{
    compact::{OpenAiResponsesCompactor, OpenAiResponsesCompactorConfig},
    provider::chatgpt_responses_provider,
};
use std::sync::Arc;

impl AgentSessionRuntimeBuilder {
    pub fn with_chatgpt_responses_provider(
        self,
        provider_id: impl Into<String>,
        model: impl Into<String>,
        auth_provider: Arc<dyn HttpAuthProvider>,
    ) -> CoreResult<Self> {
        let provider = chatgpt_responses_provider(provider_id, model, auth_provider)?;
        Ok(self.with_model_provider(Arc::new(provider)))
    }

    pub fn with_openai_responses_compactor(
        self,
        context_config: ContextCompactionConfig,
        compactor_config: OpenAiResponsesCompactorConfig,
    ) -> CoreResult<Self> {
        let compactor = OpenAiResponsesCompactor::new(compactor_config)?;
        Ok(self.with_context_compactor(context_config, Arc::new(compactor)))
    }

    pub fn with_openai_responses_compactor_estimator(
        self,
        context_config: ContextCompactionConfig,
        compactor_config: OpenAiResponsesCompactorConfig,
        estimator: Arc<dyn TokenEstimator>,
    ) -> CoreResult<Self> {
        let compactor = OpenAiResponsesCompactor::new(compactor_config)?;
        Ok(self.with_context_compactor_estimator(context_config, Arc::new(compactor), estimator))
    }
}

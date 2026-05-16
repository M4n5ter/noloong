use crate::{
    AgentSystemPrompt, BuiltInSystemPromptProfile, Locale, SystemPromptAddition, SystemPromptSource,
};
use serde::{Deserialize, Serialize};

pub const BUILT_IN_SYSTEM_PROMPT_HOOK_ID: &str = "noloong.builtin.system-prompt";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemPromptModelContext {
    pub provider_id: String,
    pub model_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSystemPrompt {
    pub source: SystemPromptSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configured_profile: Option<BuiltInSystemPromptProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_profile: Option<BuiltInSystemPromptProfile>,
    pub locale: Locale,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<SystemPromptModelContext>,
    pub base_text: String,
    #[serde(default)]
    pub additions: Vec<SystemPromptAddition>,
    #[serde(default)]
    pub enabled_addition_ids: Vec<String>,
    pub effective_text: String,
}

pub fn built_in_system_prompt(locale: Locale) -> &'static str {
    built_in_system_prompt_for_profile(locale, BuiltInSystemPromptProfile::General)
}

pub fn built_in_system_prompt_for_profile(
    locale: Locale,
    profile: BuiltInSystemPromptProfile,
) -> &'static str {
    match (locale, profile) {
        (Locale::En, BuiltInSystemPromptProfile::OpenAi) => {
            include_str!("prompts/system.openai.en.md")
        }
        (Locale::Zh, BuiltInSystemPromptProfile::OpenAi) => {
            include_str!("prompts/system.openai.zh.md")
        }
        (Locale::En, _) => include_str!("prompts/system.general.en.md"),
        (Locale::Zh, _) => include_str!("prompts/system.general.zh.md"),
    }
}

pub fn resolve_system_prompt(
    locale: Locale,
    prompt: &AgentSystemPrompt,
    model: Option<&SystemPromptModelContext>,
) -> ResolvedSystemPrompt {
    match prompt {
        AgentSystemPrompt::BuiltIn { profile, additions } => {
            let resolved_profile = resolve_built_in_profile(*profile, model);
            let base_text = built_in_system_prompt_for_profile(locale, resolved_profile).to_owned();
            resolved_prompt(
                prompt.source(),
                Some(*profile),
                Some(resolved_profile),
                locale,
                model,
                base_text,
                additions.clone(),
            )
        }
        AgentSystemPrompt::Custom { prompt, additions } => resolved_prompt(
            SystemPromptSource::Custom,
            None,
            None,
            locale,
            model,
            prompt.clone(),
            additions.clone(),
        ),
    }
}

pub fn resolve_built_in_profile(
    profile: BuiltInSystemPromptProfile,
    model: Option<&SystemPromptModelContext>,
) -> BuiltInSystemPromptProfile {
    match profile {
        BuiltInSystemPromptProfile::Auto => {
            if model.map(is_openai_model_context).unwrap_or(false) {
                BuiltInSystemPromptProfile::OpenAi
            } else {
                BuiltInSystemPromptProfile::General
            }
        }
        explicit => explicit,
    }
}

fn resolved_prompt(
    source: SystemPromptSource,
    configured_profile: Option<BuiltInSystemPromptProfile>,
    resolved_profile: Option<BuiltInSystemPromptProfile>,
    locale: Locale,
    model: Option<&SystemPromptModelContext>,
    base_text: String,
    additions: Vec<SystemPromptAddition>,
) -> ResolvedSystemPrompt {
    let enabled_addition_ids = additions
        .iter()
        .filter(|addition| addition.enabled)
        .map(|addition| addition.id.clone())
        .collect::<Vec<_>>();
    let effective_text = render_effective_system_prompt(&base_text, &additions);
    ResolvedSystemPrompt {
        source,
        configured_profile,
        resolved_profile,
        locale,
        model: model.cloned(),
        base_text,
        additions,
        enabled_addition_ids,
        effective_text,
    }
}

fn render_effective_system_prompt(base_text: &str, additions: &[SystemPromptAddition]) -> String {
    let mut enabled = additions
        .iter()
        .filter(|addition| addition.enabled)
        .peekable();
    if enabled.peek().is_none() {
        return base_text.to_owned();
    }

    let mut text = base_text.trim_end().to_owned();
    text.push_str("\n\n<system_prompt_additions>\n");
    for addition in enabled {
        text.push_str("<addition id=\"");
        text.push_str(&escape_xml_attribute(&addition.id));
        text.push_str("\">\n");
        text.push_str(addition.text.trim());
        text.push_str("\n</addition>\n");
    }
    text.push_str("</system_prompt_additions>\n");
    text
}

pub(crate) fn escape_xml_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn is_openai_model_context(model: &SystemPromptModelContext) -> bool {
    is_openai_provider(&model.provider_id) || is_openai_model_name(&model.model_name)
}

fn is_openai_provider(provider_id: &str) -> bool {
    let normalized = provider_id.to_ascii_lowercase().replace('-', "_");
    matches!(normalized.as_str(), "openai" | "chatgpt_responses")
}

fn is_openai_model_name(model_name: &str) -> bool {
    let normalized = model_name.to_ascii_lowercase().replace('_', "-");
    normalized.starts_with("gpt-")
}

use noloong_config::{
    BuiltInProviderConfig, ChatGptAuthConfig, CliConfigError, HostProfileConfig, Locale,
    ProfileCompactionConfig, ProfileEventStoreConfig, ResponsesProviderReasoningConfig,
    ResponsesProviderReasoningEffort, ResponsesProviderReasoningSummary, ResponsesStateMode,
    RuntimeProfileConfig, resolve_profile_config_path,
    schema::{ProfileConfigSchemaIndex, ProfileConfigValidator},
    starter_profile_config,
};
use std::path::PathBuf;
use thiserror::Error;

use crate::chat::ChatSessionStore;
use crate::interaction::{AppInteractionEndpoint, AppInteractionStatus};

mod chat_runtime;
mod helpers;
mod integrations;
#[cfg(test)]
mod tests;
mod types;

pub use chat_runtime::ChatEmptyState;
use helpers::*;
pub use types::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppLaunchOptions {
    pub profile_config_path: Option<String>,
    pub locale: Option<Locale>,
    pub interaction_endpoint: Option<AppInteractionEndpoint>,
    pub interaction_status: Option<AppInteractionStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRoute {
    Chat,
    Tools,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppStatus {
    StarterDraft,
    Loaded,
    Dirty,
    Valid,
    Invalid(Vec<String>),
    Saved,
    SaveFailed(String),
}

#[derive(Clone, Debug)]
pub struct AppViewModel {
    pub config_path: PathBuf,
    pub config: HostProfileConfig,
    pub schema_index: ProfileConfigSchemaIndex,
    schema_validator: ProfileConfigValidator,
    pub locale: Locale,
    pub interaction_endpoint: Option<AppInteractionEndpoint>,
    pub interaction_status: AppInteractionStatus,
    pub chat: ChatSessionStore,
    pub route: AppRoute,
    pub jsonc_open: bool,
    pub jsonc_text: String,
    pub jsonc_error: Option<String>,
    pub selected_profile_id: Option<String>,
    pub status: AppStatus,
}

impl AppViewModel {
    pub fn load(options: AppLaunchOptions) -> Result<Self, AppError> {
        let config_path = resolve_profile_config_path(options.profile_config_path.as_deref())?;
        let missing = !config_path.exists();
        let config = if missing {
            starter_profile_config()
        } else {
            HostProfileConfig::load(&config_path)?
        };
        config.validate()?;
        let selected_profile_id = config.default_profile_id.clone().or_else(|| {
            config
                .profiles
                .first()
                .map(|profile| profile.profile_id.clone())
        });
        let jsonc_text = config.to_canonical_json()?;
        let interaction_endpoint = options.interaction_endpoint;
        let interaction_status = options.interaction_status.unwrap_or_else(|| {
            if interaction_endpoint.is_some() {
                AppInteractionStatus::Pending
            } else {
                AppInteractionStatus::Unavailable
            }
        });
        Ok(Self {
            config_path,
            config,
            schema_index: ProfileConfigSchemaIndex::new(),
            schema_validator: ProfileConfigValidator::new()?,
            locale: options.locale.unwrap_or_else(Locale::detect),
            interaction_endpoint,
            interaction_status,
            chat: ChatSessionStore::default(),
            route: AppRoute::Chat,
            jsonc_open: false,
            jsonc_text,
            jsonc_error: None,
            selected_profile_id,
            status: if missing {
                AppStatus::StarterDraft
            } else {
                AppStatus::Loaded
            },
        })
    }

    pub fn validate(&mut self) -> bool {
        if let Some(error) = self.jsonc_error.clone() {
            self.status = AppStatus::Invalid(vec![error]);
            return false;
        }
        match self.config.validate() {
            Ok(()) => {
                self.status = AppStatus::Valid;
                true
            }
            Err(error) => {
                self.status = AppStatus::Invalid(vec![error.to_string()]);
                false
            }
        }
    }

    pub fn save(&mut self) -> Result<(), AppError> {
        if let Some(error) = self.jsonc_error.clone() {
            self.status = AppStatus::Invalid(vec![error.clone()]);
            return Err(AppError::InvalidJsonc(error));
        }
        self.config.validate()?;
        self.config.save_canonical(&self.config_path)?;
        self.sync_jsonc_from_config()?;
        self.status = AppStatus::Saved;
        Ok(())
    }

    #[cfg(test)]
    fn jsonc_preview(&self) -> Result<String, AppError> {
        if self.jsonc_error.is_some() {
            Ok(self.jsonc_text.clone())
        } else {
            Ok(self.config.to_canonical_json()?)
        }
    }

    pub fn format_jsonc(&mut self) -> Result<(), AppError> {
        if let Some(error) = self.jsonc_error.clone() {
            return Err(AppError::InvalidJsonc(error));
        }
        self.sync_jsonc_from_config()?;
        Ok(())
    }

    pub fn set_jsonc_text(&mut self, text: String) -> bool {
        self.jsonc_text = text;
        match self.schema_validator.parse_text(&self.jsonc_text) {
            Ok(config) => {
                self.config = config;
                self.jsonc_error = None;
                self.status = AppStatus::Dirty;
                self.ensure_selected_profile();
                true
            }
            Err(error) => {
                let error = error.to_string();
                self.jsonc_error = Some(error.clone());
                self.status = AppStatus::Invalid(vec![error]);
                false
            }
        }
    }

    pub fn is_settings_form_read_only(&self) -> bool {
        self.jsonc_error.is_some()
    }

    pub fn jsonc_error(&self) -> Option<&str> {
        self.jsonc_error.as_deref()
    }

    pub fn selected_profile(&self) -> Option<&noloong_config::RuntimeProfileConfig> {
        self.config
            .selected_profile(self.selected_profile_id.as_deref())
    }

    pub fn selected_profile_mut(&mut self) -> Option<&mut noloong_config::RuntimeProfileConfig> {
        self.config
            .selected_profile_mut(self.selected_profile_id.as_deref())
    }

    pub fn provider_summaries(&self) -> Vec<ProfileProviderSummary> {
        self.config
            .profiles
            .iter()
            .map(|profile| {
                let is_active =
                    self.config.default_profile_id.as_deref() == Some(profile.profile_id.as_str());
                let is_selected =
                    self.selected_profile_id.as_deref() == Some(profile.profile_id.as_str());
                ProfileProviderSummary {
                    profile_id: profile.profile_id.clone(),
                    display_name: profile.display_name.clone(),
                    provider_type: profile.provider.type_tag().into(),
                    model: profile.provider.model().into(),
                    is_active,
                    is_selected,
                }
            })
            .collect()
    }

    pub fn select_provider_profile(&mut self, profile_id: String) {
        if self
            .config
            .profiles
            .iter()
            .any(|profile| profile.profile_id == profile_id)
        {
            self.selected_profile_id = Some(profile_id);
        }
    }

    pub fn activate_selected_provider_profile(&mut self) {
        let Some(profile_id) = self.selected_profile_id.clone() else {
            return;
        };
        if self
            .config
            .profiles
            .iter()
            .any(|profile| profile.profile_id == profile_id)
        {
            self.config.default_profile_id = Some(profile_id);
            self.mark_dirty_from_form();
        }
    }

    pub fn add_provider_profile(&mut self) -> String {
        let next = self.config.profiles.len() + 1;
        let profile_id = self.unique_profile_id(&format!("provider-{next}"));
        let display_name = format!("Provider {next}");
        let profile = RuntimeProfileConfig {
            profile_id: profile_id.clone(),
            display_name,
            description: None,
            provider: BuiltInProviderConfig::ChatgptResponses {
                provider_id: None,
                model: "gpt-5.4-mini".into(),
                auth: ChatGptAuthConfig::default(),
                state_mode: ResponsesStateMode::default(),
                reasoning: Some(ResponsesProviderReasoningConfig {
                    enabled: true,
                    effort: Some(ResponsesProviderReasoningEffort::Medium),
                    summary: Some(ResponsesProviderReasoningSummary::Auto),
                    include_encrypted: None,
                }),
                allow_file_data_url_input: true,
            },
            event_store: None,
            compaction: ProfileCompactionConfig::Auto,
            plugins: Vec::new(),
            manifest_patches: Vec::new(),
            metadata: Default::default(),
        };
        self.config.profiles.push(profile);
        self.selected_profile_id = Some(profile_id.clone());
        if self.config.default_profile_id.is_none() {
            self.config.default_profile_id = Some(profile_id.clone());
        }
        self.mark_dirty_from_form();
        profile_id
    }

    pub fn duplicate_selected_provider_profile(&mut self) -> Option<String> {
        let mut profile = self.selected_profile()?.clone();
        profile.profile_id = self.unique_profile_id(&format!("{}-copy", profile.profile_id));
        profile.display_name = format!("{} Copy", profile.display_name);
        let profile_id = profile.profile_id.clone();
        self.config.profiles.push(profile);
        self.selected_profile_id = Some(profile_id.clone());
        self.mark_dirty_from_form();
        Some(profile_id)
    }

    pub fn remove_selected_provider_profile(&mut self) -> bool {
        if self.config.profiles.len() <= 1 {
            return false;
        }
        let Some(profile_id) = self.selected_profile_id.clone() else {
            return false;
        };
        let Some(index) = self
            .config
            .profiles
            .iter()
            .position(|profile| profile.profile_id == profile_id)
        else {
            return false;
        };
        self.config.profiles.remove(index);
        let next_index = index.min(self.config.profiles.len().saturating_sub(1));
        let next_profile_id = self
            .config
            .profiles
            .get(next_index)
            .map(|profile| profile.profile_id.clone());
        if self.config.default_profile_id.as_deref() == Some(profile_id.as_str()) {
            self.config.default_profile_id = next_profile_id.clone();
        }
        self.selected_profile_id = next_profile_id;
        self.mark_dirty_from_form();
        true
    }

    pub fn select_route(&mut self, route: AppRoute) {
        self.route = route;
    }

    pub fn toggle_jsonc(&mut self) -> Result<(), AppError> {
        self.jsonc_open = !self.jsonc_open;
        if self.jsonc_open && self.jsonc_error.is_none() {
            self.sync_jsonc_from_config()?;
        }
        Ok(())
    }

    pub fn close_jsonc(&mut self) {
        self.jsonc_open = false;
    }

    pub fn has_interaction_endpoint(&self) -> bool {
        self.interaction_endpoint.is_some()
    }

    pub fn set_display_name(&mut self, value: String) {
        if let Some(profile) = self.selected_profile_mut() {
            profile.display_name = value;
            self.mark_dirty_from_form();
        }
    }

    pub fn set_description(&mut self, value: String) {
        if let Some(profile) = self.selected_profile_mut() {
            profile.description = optional_string(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_model(&mut self, value: String) {
        if let Some(profile) = self.selected_profile_mut() {
            *profile.provider.model_mut() = value;
            self.mark_dirty_from_form();
        }
    }

    pub fn set_provider_id(&mut self, value: String) {
        if let Some(provider) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.provider)
        {
            *provider_id_mut(provider) = optional_string(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_base_url(&mut self, value: String) {
        if let Some(provider) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.provider)
            && let Some(field) = base_url_mut(provider)
        {
            *field = optional_string(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_api_key_env(&mut self, value: String) {
        if let Some(provider) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.provider)
            && let Some(field) = api_key_env_mut(provider)
        {
            *field = optional_string(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_max_tokens_text(&mut self, value: String) {
        if let Some(provider) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.provider)
            && let Some(field) = max_tokens_mut(provider)
        {
            *field = optional_u64(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn toggle_file_data_url_input(&mut self) {
        if let Some(provider) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.provider)
            && let Some(field) = allow_file_data_url_input_mut(provider)
        {
            *field = !*field;
            self.mark_dirty_from_form();
        }
    }

    pub fn toggle_reasoning_enabled(&mut self) {
        if let Some(provider) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.provider)
            && let Some(reasoning) = responses_reasoning_mut(provider)
        {
            reasoning.enabled = !reasoning.enabled;
            self.mark_dirty_from_form();
        }
    }

    pub fn set_reasoning_effort(&mut self, effort: &str) {
        if let Some(reasoning) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.provider)
            .and_then(responses_reasoning_mut)
        {
            reasoning.effort = parse_responses_reasoning_effort(effort);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_reasoning_summary(&mut self, summary: &str) {
        if let Some(reasoning) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.provider)
            .and_then(responses_reasoning_mut)
        {
            reasoning.summary = parse_responses_reasoning_summary(summary);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_event_store_sqlite_url(&mut self, value: String) {
        if let Some(profile) = self.selected_profile_mut() {
            profile.event_store =
                optional_string(value).map(|database_url| ProfileEventStoreConfig::Sqlite {
                    database_url,
                    migrate_on_connect: true,
                });
            self.mark_dirty_from_form();
        }
    }

    pub fn set_locale(&mut self, locale: Locale) {
        if let Some(profile) = self.selected_profile_mut() {
            profile
                .manifest_patches
                .retain(|patch| !matches!(patch, noloong_config::ManifestPatch::SetLocale { .. }));
            profile
                .manifest_patches
                .push(noloong_config::ManifestPatch::SetLocale { locale });
            self.mark_dirty_from_form();
        }
    }

    pub fn provider_type(&self) -> String {
        self.selected_profile()
            .map(|profile| profile.provider.type_tag())
            .unwrap_or("")
            .into()
    }

    pub fn provider_id(&self) -> String {
        self.selected_profile()
            .and_then(|profile| provider_id(&profile.provider))
            .unwrap_or_default()
    }

    pub fn base_url(&self) -> String {
        self.selected_profile()
            .and_then(|profile| base_url(&profile.provider))
            .unwrap_or_default()
    }

    pub fn api_key_env(&self) -> String {
        self.selected_profile()
            .and_then(|profile| api_key_env(&profile.provider))
            .unwrap_or_default()
    }

    pub fn provider_state_mode(&self) -> Option<String> {
        self.selected_profile()
            .and_then(|profile| state_mode(&profile.provider))
            .map(str::to_string)
    }

    pub fn provider_auth_summary(&self) -> Option<String> {
        self.selected_profile()
            .and_then(|profile| match &profile.provider {
                BuiltInProviderConfig::ChatgptResponses { auth, .. } => Some(match auth {
                    ChatGptAuthConfig::TokenFile {
                        token_file,
                        token_file_env,
                    } => token_file
                        .as_deref()
                        .or(token_file_env.as_deref())
                        .unwrap_or("token_file")
                        .to_string(),
                    ChatGptAuthConfig::EnvHeaders { id, .. } => format!("env_headers:{id}"),
                }),
                _ => None,
            })
    }

    pub fn file_data_url_input(&self) -> Option<bool> {
        self.selected_profile()
            .and_then(|profile| allow_file_data_url_input(&profile.provider))
    }

    pub fn supports_base_url(&self) -> bool {
        self.selected_profile().is_some_and(|profile| {
            base_url(&profile.provider).is_some() || base_url_supported(&profile.provider)
        })
    }

    pub fn supports_api_key_env(&self) -> bool {
        self.selected_profile().is_some_and(|profile| {
            api_key_env(&profile.provider).is_some() || api_key_env_supported(&profile.provider)
        })
    }

    pub fn supports_max_tokens(&self) -> bool {
        self.selected_profile()
            .is_some_and(|profile| max_tokens_supported(&profile.provider))
    }

    pub fn max_tokens_text(&self) -> String {
        self.selected_profile()
            .and_then(|profile| max_tokens(&profile.provider))
            .map(|value| value.to_string())
            .unwrap_or_default()
    }

    pub fn reasoning_summary(&self) -> Option<ReasoningSummary> {
        self.selected_profile()
            .and_then(|profile| reasoning_summary(&profile.provider))
    }

    pub fn model(&self) -> String {
        self.selected_profile()
            .map(|profile| profile.provider.model().to_string())
            .unwrap_or_default()
    }

    pub fn selected_description(&self) -> String {
        self.selected_profile()
            .and_then(|profile| profile.description.clone())
            .unwrap_or_default()
    }

    pub fn compaction_edit(&self) -> CompactionEdit {
        let Some(profile) = self.selected_profile() else {
            return CompactionEdit::default();
        };
        match &profile.compaction {
            ProfileCompactionConfig::Auto => CompactionEdit {
                mode: "auto".into(),
                ..Default::default()
            },
            ProfileCompactionConfig::None => CompactionEdit {
                mode: "none".into(),
                ..Default::default()
            },
            ProfileCompactionConfig::OpenaiResponses {
                id,
                input_limit_model,
                compact_model,
                input_limit_tokens,
                trigger_ratio,
                summary_budget_tokens,
                keep_recent_tokens,
                mode,
                request_timeout_secs,
            } => CompactionEdit {
                mode: "openai_responses".into(),
                id: id.clone().unwrap_or_default(),
                input_limit_model: input_limit_model.clone().unwrap_or_default(),
                compact_model: compact_model.clone().unwrap_or_default(),
                input_limit_tokens: input_limit_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                trigger_ratio: trigger_ratio
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                summary_budget_tokens: summary_budget_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                keep_recent_tokens: keep_recent_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                state_mode: mode
                    .map(context_compaction_mode_as_str)
                    .unwrap_or_default()
                    .into(),
                request_timeout_secs: request_timeout_secs
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
            },
        }
    }

    pub fn set_compaction_mode(&mut self, mode: &str) {
        let Some(profile) = self.selected_profile_mut() else {
            return;
        };
        profile.compaction = match mode {
            "none" => ProfileCompactionConfig::None,
            "openai_responses" => ProfileCompactionConfig::OpenaiResponses {
                id: None,
                input_limit_model: None,
                compact_model: None,
                input_limit_tokens: None,
                trigger_ratio: None,
                summary_budget_tokens: None,
                keep_recent_tokens: None,
                mode: None,
                request_timeout_secs: None,
            },
            _ => ProfileCompactionConfig::Auto,
        };
        self.mark_dirty_from_form();
    }

    pub fn set_compaction_id(&mut self, value: String) {
        if let Some(ProfileCompactionConfig::OpenaiResponses { id, .. }) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.compaction)
        {
            *id = optional_string(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_compaction_input_limit_model(&mut self, value: String) {
        if let Some(ProfileCompactionConfig::OpenaiResponses {
            input_limit_model, ..
        }) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.compaction)
        {
            *input_limit_model = optional_string(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_compaction_compact_model(&mut self, value: String) {
        if let Some(ProfileCompactionConfig::OpenaiResponses { compact_model, .. }) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.compaction)
        {
            *compact_model = optional_string(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_compaction_input_limit_tokens(&mut self, value: String) {
        if let Some(ProfileCompactionConfig::OpenaiResponses {
            input_limit_tokens, ..
        }) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.compaction)
        {
            *input_limit_tokens = optional_u64(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_compaction_trigger_ratio(&mut self, value: String) {
        if let Some(ProfileCompactionConfig::OpenaiResponses { trigger_ratio, .. }) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.compaction)
        {
            *trigger_ratio = optional_f64(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_compaction_summary_budget_tokens(&mut self, value: String) {
        if let Some(ProfileCompactionConfig::OpenaiResponses {
            summary_budget_tokens,
            ..
        }) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.compaction)
        {
            *summary_budget_tokens = optional_u64(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_compaction_keep_recent_tokens(&mut self, value: String) {
        if let Some(ProfileCompactionConfig::OpenaiResponses {
            keep_recent_tokens, ..
        }) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.compaction)
        {
            *keep_recent_tokens = optional_u64(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_compaction_state_mode(&mut self, value: &str) {
        if let Some(ProfileCompactionConfig::OpenaiResponses { mode, .. }) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.compaction)
        {
            *mode = parse_context_compaction_mode(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn set_compaction_timeout(&mut self, value: String) {
        if let Some(ProfileCompactionConfig::OpenaiResponses {
            request_timeout_secs,
            ..
        }) = self
            .selected_profile_mut()
            .map(|profile| &mut profile.compaction)
        {
            *request_timeout_secs = optional_u64(value);
            self.mark_dirty_from_form();
        }
    }

    pub fn event_store_summary(&self) -> String {
        self.selected_profile()
            .and_then(|profile| profile.event_store.as_ref())
            .map(|store| match store {
                ProfileEventStoreConfig::Memory => "memory".into(),
                ProfileEventStoreConfig::Sqlite {
                    database_url,
                    migrate_on_connect,
                } => format!(
                    "sqlite: {database_url} ({})",
                    if *migrate_on_connect {
                        "migrate"
                    } else {
                        "no migrations"
                    }
                ),
            })
            .unwrap_or_else(|| "default".into())
    }

    pub fn event_store_sqlite_url(&self) -> String {
        self.selected_profile()
            .and_then(|profile| match profile.event_store.as_ref() {
                Some(ProfileEventStoreConfig::Sqlite { database_url, .. }) => {
                    Some(database_url.clone())
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    pub fn registry_store_summary(&self) -> String {
        self.config
            .registry_store
            .as_ref()
            .map(registry_store_summary)
            .unwrap_or_else(|| "default".into())
    }

    fn mark_dirty_from_form(&mut self) {
        self.status = AppStatus::Dirty;
        self.jsonc_error = None;
        if self.jsonc_open {
            self.sync_jsonc_from_config()
                .expect("typed profile config is serializable");
        }
    }

    fn sync_jsonc_from_config(&mut self) -> Result<(), AppError> {
        self.jsonc_text = self.config.to_canonical_json()?;
        self.jsonc_error = None;
        Ok(())
    }

    fn ensure_selected_profile(&mut self) {
        if self.selected_profile_id.as_ref().is_some_and(|id| {
            self.config
                .profiles
                .iter()
                .any(|profile| &profile.profile_id == id)
        }) {
            return;
        }
        self.selected_profile_id = self.config.default_profile_id.clone().or_else(|| {
            self.config
                .profiles
                .first()
                .map(|profile| profile.profile_id.clone())
        });
    }

    fn unique_profile_id(&self, base: &str) -> String {
        let base = sanitize_profile_id(base);
        if !self
            .config
            .profiles
            .iter()
            .any(|profile| profile.profile_id == base)
        {
            return base;
        }
        for suffix in 2.. {
            let candidate = format!("{base}-{suffix}");
            if !self
                .config
                .profiles
                .iter()
                .any(|profile| profile.profile_id == candidate)
            {
                return candidate;
            }
        }
        unreachable!("unbounded suffix search returns before overflow")
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Config(#[from] CliConfigError),
    #[error("failed to read app file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to launch app bundle: {0}")]
    Launch(String),
    #[error("cannot locate the home directory for the app bundle")]
    MissingHome,
    #[error("JSONC is invalid: {0}")]
    InvalidJsonc(String),
    #[error("interaction failed: {0}")]
    Interaction(String),
}

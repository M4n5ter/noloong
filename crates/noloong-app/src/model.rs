use noloong_agent::Locale;
use noloong_config::{
    CliConfigError, HostProfileConfig, resolve_profile_config_path,
    schema::{ProfileConfigSchemaIndex, ProfileConfigValidator},
    starter_profile_config,
};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppLaunchOptions {
    pub profile_config_path: Option<String>,
    pub locale: Option<Locale>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRoute {
    Chat,
    Profile,
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
        Ok(Self {
            config_path,
            config,
            schema_index: ProfileConfigSchemaIndex::new(),
            schema_validator: ProfileConfigValidator::new()?,
            locale: options.locale.unwrap_or_else(Locale::detect),
            route: AppRoute::Profile,
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

    pub fn is_profile_form_read_only(&self) -> bool {
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

    pub fn set_profile_id(&mut self, value: String) {
        let old_id = self.selected_profile_id.clone();
        if let Some(profile) = self.selected_profile_mut() {
            profile.profile_id = value.clone();
        }
        if self.config.default_profile_id == old_id {
            self.config.default_profile_id = Some(value.clone());
        }
        self.selected_profile_id = Some(value);
        self.mark_dirty_from_form();
    }

    pub fn set_display_name(&mut self, value: String) {
        if let Some(profile) = self.selected_profile_mut() {
            profile.display_name = value;
            self.mark_dirty_from_form();
        }
    }

    pub fn set_model(&mut self, value: String) {
        if let Some(profile) = self.selected_profile_mut() {
            *profile.provider.model_mut() = value;
            self.mark_dirty_from_form();
        }
    }

    pub fn set_default_profile(&mut self, enabled: bool) {
        if enabled {
            if let Some(id) = self.selected_profile_id.clone() {
                self.config.default_profile_id = Some(id);
            }
        } else {
            self.config.default_profile_id = None;
        }
        self.mark_dirty_from_form();
    }

    pub fn is_selected_default_profile(&self) -> bool {
        self.selected_profile_id
            .as_ref()
            .is_some_and(|id| Some(id) == self.config.default_profile_id.as_ref())
    }

    pub fn provider_type(&self) -> String {
        self.selected_profile()
            .map(|profile| profile.provider.type_tag())
            .unwrap_or("")
            .into()
    }

    pub fn model(&self) -> String {
        self.selected_profile()
            .map(|profile| profile.provider.model().to_string())
            .unwrap_or_default()
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
}

#[cfg(test)]
mod tests {
    use super::{AppError, AppLaunchOptions, AppStatus, AppViewModel};
    use crate::test_support::{remove_temp_dir, temp_dir};
    use noloong_agent::Locale;
    use std::fs;

    #[test]
    fn app_loads_starter_draft_when_config_is_missing() {
        let dir = temp_dir("app-missing-config");
        let path = dir.join("profile-config.jsonc");

        let model = AppViewModel::load(AppLaunchOptions {
            profile_config_path: Some(path.display().to_string()),
            locale: Some(Locale::Zh),
        })
        .unwrap();

        assert_eq!(model.locale, Locale::Zh);
        assert_eq!(model.status, AppStatus::StarterDraft);
        assert_eq!(
            model.config.default_profile_id.as_deref(),
            Some("chatgpt-responses")
        );
        assert!(!path.exists());
        remove_temp_dir(dir);
    }

    #[test]
    fn app_saves_canonical_config() {
        let dir = temp_dir("app-save-config");
        let path = dir.join("profile-config.jsonc");
        let mut model = AppViewModel::load(AppLaunchOptions {
            profile_config_path: Some(path.display().to_string()),
            locale: Some(Locale::En),
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
        })
        .unwrap();

        let original_model = model.model();
        assert!(!model.set_jsonc_text("{ invalid".into()));

        assert_eq!(model.model(), original_model);
        assert!(model.is_profile_form_read_only());
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
        })
        .unwrap();

        assert!(!model.set_jsonc_text("{ invalid".into()));
        let fixed = model
            .jsonc_preview()
            .unwrap()
            .replace("{ invalid", &model.config.to_canonical_json().unwrap());
        assert!(model.set_jsonc_text(fixed));
        assert!(!model.is_profile_form_read_only());

        model.save().unwrap();

        let saved = fs::read_to_string(&path).unwrap();
        assert_eq!(saved, model.config.to_canonical_json().unwrap());
        assert_eq!(model.jsonc_preview().unwrap(), saved);
        remove_temp_dir(dir);
    }
}

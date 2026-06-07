use crate::{AppLaunchOptions, runtime::AppState};
use noloong_config::{
    HostProfileConfig, resolve_profile_config_path,
    schema::{ProfileConfigSchemaIndex, profile_config_schema_value},
    starter_profile_config,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppProfileConfigDocument {
    pub path: String,
    pub text: String,
    #[serde(default)]
    pub exists: bool,
    pub validation: AppProfileConfigValidationResult,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppProfileConfigValidationResult {
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<HostProfileConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_text: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppProfileConfigValidateRequest {
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppProfileConfigSaveRequest {
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppProfileConfigCompletionRequest {
    pub text: String,
    pub offset: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppProfileConfigCompletionSet {
    pub replace_start: usize,
    pub completions: Vec<AppProfileConfigCompletion>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppProfileConfigCompletion {
    pub label: String,
    pub insert_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    pub kind: AppProfileConfigCompletionKind,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppProfileConfigCompletionKind {
    Property,
    Value,
    Snippet,
}

#[tauri::command]
pub(crate) fn app_profile_config_load(
    state: tauri::State<'_, AppState>,
) -> Result<AppProfileConfigDocument, String> {
    load_profile_config_document(state.inner().launch_options())
}

#[tauri::command]
pub(crate) fn app_profile_config_validate(
    request: AppProfileConfigValidateRequest,
) -> AppProfileConfigValidationResult {
    validate_profile_config_text(&request.text)
}

#[tauri::command]
pub(crate) fn app_profile_config_save(
    state: tauri::State<'_, AppState>,
    request: AppProfileConfigSaveRequest,
) -> Result<AppProfileConfigDocument, String> {
    save_profile_config_document(state.inner().launch_options(), request)
}

#[tauri::command]
pub(crate) fn app_profile_config_schema() -> Value {
    profile_config_schema_value()
}

#[tauri::command]
pub(crate) fn app_profile_config_completions(
    request: AppProfileConfigCompletionRequest,
) -> AppProfileConfigCompletionSet {
    complete_profile_config_text(&request.text, request.offset)
}

pub fn load_profile_config_document(
    options: &AppLaunchOptions,
) -> Result<AppProfileConfigDocument, String> {
    let path = resolve_app_profile_config_path(options)?;
    load_profile_config_document_from_path(&path)
}

pub fn save_profile_config_document(
    options: &AppLaunchOptions,
    request: AppProfileConfigSaveRequest,
) -> Result<AppProfileConfigDocument, String> {
    let path = resolve_app_profile_config_path(options)?;
    let validation = validate_profile_config_text(&request.text);
    let config = validation.config.ok_or_else(|| {
        validation
            .error
            .unwrap_or_else(|| "profile config is invalid".into())
    })?;
    config
        .save_canonical(&path)
        .map_err(|error| error.to_string())?;
    load_profile_config_document_from_path(&path)
}

pub fn validate_profile_config_text(text: &str) -> AppProfileConfigValidationResult {
    match noloong_config::schema::parse_validated_profile_config_text(text) {
        Ok(config) => match config.to_canonical_json() {
            Ok(canonical_text) => AppProfileConfigValidationResult {
                valid: true,
                error: None,
                config: Some(config),
                canonical_text: Some(canonical_text),
            },
            Err(error) => invalid_validation(error.to_string()),
        },
        Err(error) => invalid_validation(error.to_string()),
    }
}

pub fn complete_profile_config_text(text: &str, offset: usize) -> AppProfileConfigCompletionSet {
    let set = ProfileConfigSchemaIndex::new().completions_for_text(text, offset);
    AppProfileConfigCompletionSet {
        replace_start: set.replace_start,
        completions: set
            .completions
            .into_iter()
            .map(|completion| AppProfileConfigCompletion {
                label: completion.label,
                insert_text: completion.insert_text,
                detail: completion.detail,
                documentation: completion.documentation,
                kind: match completion.kind {
                    noloong_config::schema::ProfileConfigSchemaCompletionKind::Property => {
                        AppProfileConfigCompletionKind::Property
                    }
                    noloong_config::schema::ProfileConfigSchemaCompletionKind::Value => {
                        AppProfileConfigCompletionKind::Value
                    }
                    noloong_config::schema::ProfileConfigSchemaCompletionKind::Snippet => {
                        AppProfileConfigCompletionKind::Snippet
                    }
                },
            })
            .collect(),
    }
}

fn load_profile_config_document_from_path(path: &Path) -> Result<AppProfileConfigDocument, String> {
    if path.exists() {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let validation = validate_profile_config_text(&text);
        return Ok(AppProfileConfigDocument {
            path: path.display().to_string(),
            text,
            exists: true,
            validation,
        });
    }

    let config = starter_profile_config();
    let text = config
        .to_canonical_json()
        .map_err(|error| error.to_string())?;
    Ok(AppProfileConfigDocument {
        path: path.display().to_string(),
        text: text.clone(),
        exists: false,
        validation: validate_profile_config_text(&text),
    })
}

fn resolve_app_profile_config_path(options: &AppLaunchOptions) -> Result<PathBuf, String> {
    resolve_profile_config_path(options.profile_config_path.as_deref())
        .map_err(|error| error.to_string())
}

fn invalid_validation(error: String) -> AppProfileConfigValidationResult {
    AppProfileConfigValidationResult {
        valid: false,
        error: Some(error),
        config: None,
        canonical_text: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn load_returns_starter_draft_when_profile_config_is_missing() {
        let path = temp_profile_path("missing");
        let document = load_profile_config_document(&launch_options(&path)).unwrap();

        assert!(!document.exists);
        assert_eq!(document.path, path.display().to_string());
        assert!(document.validation.valid);
        assert_eq!(
            document
                .validation
                .config
                .as_ref()
                .and_then(|config| config.default_profile_id.as_deref()),
            Some("chatgpt-responses")
        );
    }

    #[test]
    fn validate_reports_invalid_jsonc_without_a_typed_config() {
        let result = validate_profile_config_text(r#"{ "profiles": ["#);

        assert!(!result.valid);
        assert!(result.config.is_none());
        assert!(
            result
                .error
                .as_deref()
                .unwrap_or_default()
                .contains("failed to parse profile config")
        );
    }

    #[test]
    fn save_writes_canonical_profile_config() {
        let path = temp_profile_path("save");
        let mut config = starter_profile_config();
        config.profiles[0].display_name = "Saved Profile".into();
        let text = config.to_canonical_json().unwrap();

        let document = save_profile_config_document(
            &launch_options(&path),
            AppProfileConfigSaveRequest { text },
        )
        .unwrap();

        assert!(document.exists);
        assert!(document.validation.valid);
        assert!(document.text.contains("\"displayName\": \"Saved Profile\""));
        assert_eq!(fs::read_to_string(path).unwrap(), document.text);
    }

    #[test]
    fn completions_include_schema_properties() {
        let text = r#"{ "profiles": [{ "provider": { "type": "chatgpt_responses" }, "c"#;
        let completions = complete_profile_config_text(text, text.len());

        assert!(
            completions
                .completions
                .iter()
                .any(|item| item.label == "compaction")
        );
    }

    fn launch_options(path: &Path) -> AppLaunchOptions {
        AppLaunchOptions {
            profile_config_path: Some(path.display().to_string()),
            ..AppLaunchOptions::default()
        }
    }

    fn temp_profile_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "noloong-app-profile-config-{name}-{}-{nanos}.jsonc",
            std::process::id()
        ))
    }
}

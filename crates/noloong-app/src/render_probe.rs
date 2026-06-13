use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};
use thiserror::Error;

pub const RENDER_PROBE_PATH_ENV: &str = "NOLOONG_RENDER_PROBE_PATH";

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRenderProbeReport {
    pub surface: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub desktop_media_query: bool,
    pub primary_heading: Option<String>,
    pub primary_heading_rect: Option<ElementRect>,
    pub surface_rect: Option<ElementRect>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Error)]
pub enum AppRenderProbeError {
    #[error("render probe write failed: {0}")]
    Write(String),
    #[error("render probe serialization failed: {0}")]
    Serialize(String),
}

#[tauri::command]
pub(crate) fn app_render_probe_report(report: AppRenderProbeReport) -> Result<(), String> {
    write_render_probe_report(report).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn app_render_probe_enabled() -> bool {
    render_probe_enabled()
}

pub fn write_render_probe_report(report: AppRenderProbeReport) -> Result<(), AppRenderProbeError> {
    let Some(path) = render_probe_path() else {
        return Ok(());
    };
    write_render_probe_report_to_path(&report, path)
}

fn render_probe_enabled() -> bool {
    render_probe_path().is_some()
}

fn write_render_probe_report_to_path(
    report: &AppRenderProbeReport,
    path: PathBuf,
) -> Result<(), AppRenderProbeError> {
    let content = serde_json::to_string_pretty(&report)
        .map_err(|error| AppRenderProbeError::Serialize(error.to_string()))?;
    fs::write(&path, content).map_err(|error| AppRenderProbeError::Write(error.to_string()))
}

fn render_probe_path() -> Option<PathBuf> {
    env::var_os(RENDER_PROBE_PATH_ENV).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn render_probe_env_name_is_stable() {
        assert_eq!(RENDER_PROBE_PATH_ENV, "NOLOONG_RENDER_PROBE_PATH");
    }

    #[test]
    fn render_probe_is_disabled_without_path() {
        let original = std::env::var_os(RENDER_PROBE_PATH_ENV);
        unsafe {
            std::env::remove_var(RENDER_PROBE_PATH_ENV);
        }
        assert!(!render_probe_enabled());
        if let Some(value) = original {
            unsafe {
                std::env::set_var(RENDER_PROBE_PATH_ENV, value);
            }
        }
    }

    #[test]
    fn render_probe_report_writes_to_path() {
        let path = std::env::temp_dir().join(format!(
            "noloong-render-probe-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));

        let report = AppRenderProbeReport {
            surface: "chat".into(),
            viewport_width: 1440,
            viewport_height: 868,
            desktop_media_query: true,
            primary_heading: Some("What should change next?".into()),
            primary_heading_rect: Some(ElementRect {
                x: 210.0,
                y: 178.0,
                width: 858.0,
                height: 67.0,
            }),
            surface_rect: None,
        };
        write_render_probe_report_to_path(&report, path.clone())
            .expect("render probe should write report");

        let written = fs::read_to_string(&path).expect("render probe report should exist");
        let parsed: AppRenderProbeReport =
            serde_json::from_str(&written).expect("render probe report should be JSON");
        assert_eq!(parsed.surface, "chat");
        assert_eq!(parsed.viewport_width, 1440);
        let _ = fs::remove_file(&path);
    }
}

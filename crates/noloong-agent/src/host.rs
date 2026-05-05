use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostEnvironment {
    pub os: String,
    pub arch: String,
    pub cwd: PathBuf,
    pub default_shell: String,
    pub available_shell_hints: Vec<String>,
    pub path_style: PathStyle,
    pub locale: Locale,
}

impl HostEnvironment {
    pub fn detect(locale_override: Option<Locale>) -> Self {
        let locale = locale_override.unwrap_or_else(Locale::detect);
        Self {
            os: env::consts::OS.to_string(),
            arch: env::consts::ARCH.to_string(),
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            default_shell: default_shell(),
            available_shell_hints: available_shell_hints(),
            path_style: PathStyle::detect(),
            locale,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathStyle {
    Unix,
    Windows,
}

impl PathStyle {
    pub fn detect() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    En,
    Zh,
}

impl Locale {
    pub fn detect() -> Self {
        for name in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = env::var(name)
                && let Some(locale) = Self::parse(&value)
            {
                return locale;
            }
        }
        Self::En
    }

    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return None;
        }
        if normalized.starts_with("zh") {
            return Some(Self::Zh);
        }
        if normalized.starts_with("en") || normalized == "c" || normalized == "posix" {
            return Some(Self::En);
        }
        None
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh",
        }
    }
}

pub(crate) fn default_shell() -> String {
    if cfg!(windows) {
        env::var("COMSPEC").unwrap_or_else(|_| "powershell".into())
    } else {
        env::var("SHELL").unwrap_or_else(|_| "sh".into())
    }
}

fn available_shell_hints() -> Vec<String> {
    if cfg!(windows) {
        vec!["powershell".into(), "cmd".into()]
    } else {
        vec!["sh".into(), "bash".into(), "zsh".into()]
    }
}

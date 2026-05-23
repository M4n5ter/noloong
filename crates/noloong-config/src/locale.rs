use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
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

    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh",
        }
    }
}

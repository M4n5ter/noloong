use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ThinkingKind {
    #[default]
    Raw,
    Summary,
    Redacted,
    Encrypted,
    Custom(String),
}

impl ThinkingKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Raw => "raw",
            Self::Summary => "summary",
            Self::Redacted => "redacted",
            Self::Encrypted => "encrypted",
            Self::Custom(kind) => kind,
        }
    }
}

impl Serialize for ThinkingKind {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ThinkingKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let kind = String::deserialize(deserializer)?;
        Ok(match kind.as_str() {
            "raw" => Self::Raw,
            "summary" => Self::Summary,
            "redacted" => Self::Redacted,
            "encrypted" => Self::Encrypted,
            _ => Self::Custom(kind),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingBlock {
    #[serde(default)]
    pub kind: ThinkingKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_descriptor: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl ThinkingBlock {
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            kind: ThinkingKind::Raw,
            text: Some(text.into()),
            raw: None,
            replay_descriptor: None,
            metadata: Map::new(),
        }
    }

    pub fn from_delta(delta: &ThinkingDelta) -> Self {
        let mut block = Self {
            kind: delta.kind.clone(),
            text: None,
            raw: None,
            replay_descriptor: None,
            metadata: Map::new(),
        };
        block.apply_delta(delta);
        block
    }

    pub fn apply_delta(&mut self, delta: &ThinkingDelta) {
        if let Some(text_delta) = &delta.text_delta
            && !text_delta.is_empty()
        {
            self.text
                .get_or_insert_with(String::new)
                .push_str(text_delta);
        }
        if let Some(raw_snapshot) = &delta.raw_snapshot {
            self.raw = Some(raw_snapshot.clone());
        }
        if let Some(replay_descriptor) = &delta.replay_descriptor {
            self.replay_descriptor = Some(replay_descriptor.clone());
        }
        self.metadata.extend(delta.metadata.clone());
    }

    pub fn is_empty(&self) -> bool {
        self.text.as_ref().is_none_or(String::is_empty)
            && self.raw.is_none()
            && self.replay_descriptor.is_none()
            && self.metadata.is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingDelta {
    #[serde(default)]
    pub kind: ThinkingKind,
    #[serde(default, alias = "text", skip_serializing_if = "Option::is_none")]
    pub text_delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_snapshot: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_descriptor: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl ThinkingDelta {
    pub fn from_text(text_delta: impl Into<String>) -> Self {
        Self {
            kind: ThinkingKind::Raw,
            text_delta: Some(text_delta.into()),
            raw_snapshot: None,
            replay_descriptor: None,
            metadata: Map::new(),
        }
    }

    pub fn from_summary(text_delta: impl Into<String>) -> Self {
        Self {
            kind: ThinkingKind::Summary,
            text_delta: Some(text_delta.into()),
            raw_snapshot: None,
            replay_descriptor: None,
            metadata: Map::new(),
        }
    }

    pub fn with_raw(mut self, raw_snapshot: Value) -> Self {
        self.raw_snapshot = Some(raw_snapshot);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.text_delta.as_ref().is_none_or(String::is_empty)
            && self.raw_snapshot.is_none()
            && self.replay_descriptor.is_none()
            && self.metadata.is_empty()
    }
}

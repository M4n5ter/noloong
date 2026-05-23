use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AppMediaKind {
    #[default]
    File,
    Image,
    Audio,
    Video,
    Custom(String),
}

impl AppMediaKind {
    pub fn from_mime_type(mime_type: Option<&str>) -> Self {
        let Some(mime_type) = mime_type else {
            return Self::File;
        };
        if mime_type.starts_with("image/") {
            Self::Image
        } else if mime_type.starts_with("audio/") {
            Self::Audio
        } else if mime_type.starts_with("video/") {
            Self::Video
        } else {
            Self::File
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::File => "file",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Custom(kind) => kind,
        }
    }
}

impl Serialize for AppMediaKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AppMediaKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let kind = String::deserialize(deserializer)?;
        Ok(match kind.as_str() {
            "file" => Self::File,
            "image" => Self::Image,
            "audio" => Self::Audio,
            "video" => Self::Video,
            _ => Self::Custom(kind),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppMediaSource {
    Uri { uri: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppMediaBlock {
    pub kind: AppMediaKind,
    pub source: AppMediaSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

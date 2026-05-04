use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MediaKind {
    #[default]
    File,
    Image,
    Audio,
    Video,
    Custom(String),
}

impl MediaKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::File => "file",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Custom(kind) => kind,
        }
    }
}

impl Serialize for MediaKind {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MediaKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MediaEncoding {
    #[default]
    Base64,
    Custom(String),
}

impl MediaEncoding {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Base64 => "base64",
            Self::Custom(encoding) => encoding,
        }
    }
}

impl Serialize for MediaEncoding {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for MediaEncoding {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoding = String::deserialize(deserializer)?;
        Ok(match encoding.as_str() {
            "base64" => Self::Base64,
            _ => Self::Custom(encoding),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSource {
    Uri {
        uri: String,
    },
    Inline {
        data: String,
        encoding: MediaEncoding,
    },
    Provider {
        #[serde(rename = "providerId")]
        provider_id: String,
        id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EncodedMediaData {
    pub data: String,
    pub encoding: MediaEncoding,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaBlock {
    pub kind: MediaKind,
    pub source: MediaSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<EncodedMediaData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_descriptor: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

impl MediaBlock {
    pub fn uri(kind: MediaKind, uri: impl Into<String>) -> Self {
        Self::new(kind, MediaSource::Uri { uri: uri.into() })
    }

    pub fn inline_base64(kind: MediaKind, data: impl Into<String>) -> Self {
        Self::new(
            kind,
            MediaSource::Inline {
                data: data.into(),
                encoding: MediaEncoding::Base64,
            },
        )
    }

    pub fn provider(
        kind: MediaKind,
        provider_id: impl Into<String>,
        id: impl Into<String>,
    ) -> Self {
        Self::new(
            kind,
            MediaSource::Provider {
                provider_id: provider_id.into(),
                id: id.into(),
            },
        )
    }

    pub fn from_delta(delta: &MediaDelta) -> Option<Self> {
        let source = delta.source.clone().or_else(|| {
            delta.data_delta.as_ref().map(|data| MediaSource::Inline {
                data: data.clone(),
                encoding: MediaEncoding::Base64,
            })
        })?;
        let mut block = Self::new(delta.kind.clone(), source);
        if delta.source.is_some()
            && let Some(data_delta) = &delta.data_delta
        {
            block.append_encoded_data(data_delta, MediaEncoding::Base64);
        }
        block.mime_type.clone_from(&delta.mime_type);
        block.name.clone_from(&delta.name);
        block.replay_descriptor.clone_from(&delta.replay_descriptor);
        block.metadata.extend(delta.metadata.clone());
        Some(block)
    }

    pub fn apply_delta(&mut self, delta: &MediaDelta) {
        if let Some(source) = &delta.source {
            if !matches!(source, MediaSource::Inline { .. }) {
                self.move_inline_source_to_data();
            }
            self.source = source.clone();
        }
        if let Some(data_delta) = &delta.data_delta
            && !data_delta.is_empty()
        {
            if let MediaSource::Inline {
                data,
                encoding: MediaEncoding::Base64,
            } = &mut self.source
                && self.data.is_none()
            {
                data.push_str(data_delta);
            } else {
                self.append_encoded_data(data_delta, MediaEncoding::Base64);
            }
        }
        if let Some(mime_type) = &delta.mime_type {
            self.mime_type = Some(mime_type.clone());
        }
        if let Some(name) = &delta.name {
            self.name = Some(name.clone());
        }
        if let Some(replay_descriptor) = &delta.replay_descriptor {
            self.replay_descriptor = Some(replay_descriptor.clone());
        }
        self.metadata.extend(delta.metadata.clone());
    }

    fn move_inline_source_to_data(&mut self) {
        let MediaSource::Inline { data, encoding } = &self.source else {
            return;
        };
        if data.is_empty() {
            return;
        }
        let data = data.clone();
        let encoding = encoding.clone();
        self.append_encoded_data(&data, encoding);
    }

    fn append_encoded_data(&mut self, data_delta: &str, encoding: MediaEncoding) {
        if data_delta.is_empty() {
            return;
        }
        match &mut self.data {
            Some(data) if data.encoding == encoding => data.data.push_str(data_delta),
            _ => {
                self.data = Some(EncodedMediaData {
                    data: data_delta.into(),
                    encoding,
                });
            }
        }
    }

    fn new(kind: MediaKind, source: MediaSource) -> Self {
        Self {
            kind,
            source,
            data: None,
            mime_type: None,
            name: None,
            replay_descriptor: None,
            metadata: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaDelta {
    pub kind: MediaKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<MediaSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_descriptor: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub done: bool,
}

impl MediaDelta {
    pub fn from_inline_base64_delta(kind: MediaKind, data_delta: impl Into<String>) -> Self {
        Self {
            kind,
            data_delta: Some(data_delta.into()),
            source: None,
            mime_type: None,
            name: None,
            replay_descriptor: None,
            metadata: Map::new(),
            done: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.data_delta.as_ref().is_none_or(String::is_empty)
            && self.source.is_none()
            && self.mime_type.is_none()
            && self.name.is_none()
            && self.replay_descriptor.is_none()
            && self.metadata.is_empty()
            && !self.done
    }
}

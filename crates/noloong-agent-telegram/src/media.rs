use crate::{
    config::TelegramFilePolicy,
    input::{TelegramAttachment, TelegramAttachmentKind},
    telegram_api::{TelegramApi, TelegramApiError, TelegramFile},
};
use base64::{Engine as _, engine::general_purpose};
use noloong_agent_core::{MediaBlock, MediaKind};
use serde_json::{Map, Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use url::Url;

const MIME_IMAGE_JPEG: &str = "image/jpeg";
const MIME_EXTENSIONS: &[(&str, &str)] = &[
    (MIME_IMAGE_JPEG, "jpg"),
    ("image/png", "png"),
    ("image/webp", "webp"),
    ("audio/mpeg", "mp3"),
    ("audio/ogg", "ogg"),
    ("video/mp4", "mp4"),
    ("application/pdf", "pdf"),
];

#[derive(Clone)]
pub struct TelegramAttachmentResolver {
    api: Arc<dyn TelegramApi>,
    policy: TelegramFilePolicy,
}

impl TelegramAttachmentResolver {
    pub fn new(api: Arc<dyn TelegramApi>, policy: TelegramFilePolicy) -> Self {
        Self { api, policy }
    }

    pub async fn resolve_all(
        &self,
        attachments: &[TelegramAttachment],
    ) -> Result<Vec<MediaBlock>, TelegramMediaResolutionError> {
        let mut blocks = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            blocks.push(self.resolve_one(attachment).await?);
        }
        Ok(blocks)
    }

    pub async fn resolve_one(
        &self,
        attachment: &TelegramAttachment,
    ) -> Result<MediaBlock, TelegramMediaResolutionError> {
        let telegram_file =
            self.api
                .get_file(&attachment.file.file_id)
                .await
                .map_err(|source| TelegramMediaResolutionError::Api {
                    file_id: attachment.file.file_id.clone(),
                    source,
                })?;
        reject_oversized(
            attachment,
            self.policy.max_download_bytes,
            known_file_size(attachment, &telegram_file),
        )?;
        let mime_type = attachment_mime_type(attachment)?;
        let file_path = telegram_file.file_path.as_deref().ok_or_else(|| {
            TelegramMediaResolutionError::MissingTelegramFilePath {
                file_id: attachment.file.file_id.clone(),
            }
        })?;
        let media_kind = media_kind(attachment.kind);
        let mut block = if should_inline(known_file_size(attachment, &telegram_file), &self.policy)
        {
            let bytes = self
                .api
                .download_file(file_path)
                .await
                .map_err(|source| map_download_error(attachment, source))?;
            reject_oversized(
                attachment,
                self.policy.max_download_bytes,
                Some(bytes.len() as u64),
            )?;
            if bytes.len() <= self.policy.inline_max_bytes {
                MediaBlock::inline_base64(media_kind, general_purpose::STANDARD.encode(bytes))
            } else {
                let path = self
                    .large_file_path(attachment, file_path, &mime_type)
                    .await?;
                write_large_bytes(&path, &bytes).await?;
                MediaBlock::uri(media_kind, file_uri(&path)?)
            }
        } else {
            let path = self
                .large_file_path(attachment, file_path, &mime_type)
                .await?;
            self.api
                .download_file_to_path(file_path, &path)
                .await
                .map_err(|source| map_download_error(attachment, source))?;
            MediaBlock::uri(media_kind, file_uri(&path)?)
        };
        block.mime_type = Some(mime_type);
        block.name = Some(display_name(attachment, file_path));
        block.replay_descriptor = Some(telegram_replay_descriptor(attachment));
        block.metadata.insert(
            "telegram".into(),
            telegram_metadata(attachment, &telegram_file),
        );
        Ok(block)
    }

    async fn large_file_path(
        &self,
        attachment: &TelegramAttachment,
        telegram_file_path: &str,
        mime_type: &str,
    ) -> Result<PathBuf, TelegramMediaResolutionError> {
        let root = self
            .policy
            .download_dir
            .clone()
            .unwrap_or_else(default_download_dir);
        let dir = root.join(sanitize_file_component(&attachment.file.file_unique_id));
        tokio::fs::create_dir_all(&dir).await.map_err(|source| {
            TelegramMediaResolutionError::Io {
                path: dir.clone(),
                source,
            }
        })?;
        let path = dir.join(storage_file_name(attachment, telegram_file_path, mime_type));
        Ok(path)
    }
}

#[derive(Debug, Error)]
pub enum TelegramMediaResolutionError {
    #[error("telegram file {file_id} is too large: limit {limit} bytes, actual {actual:?} bytes")]
    FileTooLarge {
        file_id: String,
        limit: usize,
        actual: Option<u64>,
    },
    #[error("telegram file {file_id} is missing MIME type for {kind}")]
    MissingMime { file_id: String, kind: &'static str },
    #[error("telegram file {file_id} did not include a file path")]
    MissingTelegramFilePath { file_id: String },
    #[error("telegram media API failed for {file_id}: {source}")]
    Api {
        file_id: String,
        #[source]
        source: TelegramApiError,
    },
    #[error("telegram media local file failed at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("telegram media local file path cannot be represented as file URI: {}", path.display())]
    InvalidFileUri { path: PathBuf },
}

async fn write_large_bytes(path: &Path, bytes: &[u8]) -> Result<(), TelegramMediaResolutionError> {
    tokio::fs::write(path, bytes)
        .await
        .map_err(|source| TelegramMediaResolutionError::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn known_file_size(attachment: &TelegramAttachment, telegram_file: &TelegramFile) -> Option<u64> {
    attachment.file.file_size.or(telegram_file.file_size)
}

fn should_inline(file_size: Option<u64>, policy: &TelegramFilePolicy) -> bool {
    file_size.is_none_or(|file_size| file_size <= policy.inline_max_bytes as u64)
}

fn reject_oversized(
    attachment: &TelegramAttachment,
    limit: usize,
    actual: Option<u64>,
) -> Result<(), TelegramMediaResolutionError> {
    let Some(actual) = actual else {
        return Ok(());
    };
    if actual > limit as u64 {
        return Err(TelegramMediaResolutionError::FileTooLarge {
            file_id: attachment.file.file_id.clone(),
            limit,
            actual: Some(actual),
        });
    }
    Ok(())
}

fn map_download_error(
    attachment: &TelegramAttachment,
    source: TelegramApiError,
) -> TelegramMediaResolutionError {
    match source {
        TelegramApiError::FileTooLarge { limit, actual } => {
            TelegramMediaResolutionError::FileTooLarge {
                file_id: attachment.file.file_id.clone(),
                limit,
                actual,
            }
        }
        source => TelegramMediaResolutionError::Api {
            file_id: attachment.file.file_id.clone(),
            source,
        },
    }
}

fn attachment_mime_type(
    attachment: &TelegramAttachment,
) -> Result<String, TelegramMediaResolutionError> {
    if let Some(mime_type) = attachment
        .file
        .mime_type
        .as_deref()
        .map(str::trim)
        .filter(|mime_type| !mime_type.is_empty())
    {
        return Ok(mime_type.to_owned());
    }
    match attachment.kind {
        TelegramAttachmentKind::Photo { .. } => Ok(MIME_IMAGE_JPEG.into()),
        _ => Err(TelegramMediaResolutionError::MissingMime {
            file_id: attachment.file.file_id.clone(),
            kind: attachment_kind_name(attachment.kind),
        }),
    }
}

fn media_kind(kind: TelegramAttachmentKind) -> MediaKind {
    match kind {
        TelegramAttachmentKind::Photo { .. } => MediaKind::Image,
        TelegramAttachmentKind::Document => MediaKind::File,
        TelegramAttachmentKind::Audio { .. } | TelegramAttachmentKind::Voice { .. } => {
            MediaKind::Audio
        }
        TelegramAttachmentKind::Video { .. } => MediaKind::Video,
    }
}

fn attachment_kind_name(kind: TelegramAttachmentKind) -> &'static str {
    match kind {
        TelegramAttachmentKind::Photo { .. } => "photo",
        TelegramAttachmentKind::Document => "document",
        TelegramAttachmentKind::Audio { .. } => "audio",
        TelegramAttachmentKind::Voice { .. } => "voice",
        TelegramAttachmentKind::Video { .. } => "video",
    }
}

fn display_name(attachment: &TelegramAttachment, telegram_file_path: &str) -> String {
    attachment
        .file
        .file_name
        .clone()
        .or_else(|| telegram_path_file_name(telegram_file_path).map(str::to_owned))
        .unwrap_or_else(|| attachment_kind_name(attachment.kind).to_owned())
}

fn storage_file_name(
    attachment: &TelegramAttachment,
    telegram_file_path: &str,
    mime_type: &str,
) -> String {
    let name = display_name(attachment, telegram_file_path);
    let safe = sanitize_file_component(&name);
    if Path::new(&safe).extension().is_some() {
        return safe;
    }
    match default_extension(mime_type) {
        Some(extension) => format!("{safe}.{extension}"),
        None => safe,
    }
}

fn telegram_path_file_name(path: &str) -> Option<&str> {
    path.rsplit('/').find(|part| !part.is_empty())
}

fn sanitize_file_component(value: &str) -> String {
    let safe = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .to_owned();
    if safe.is_empty() { "file".into() } else { safe }
}

fn default_extension(mime_type: &str) -> Option<&'static str> {
    MIME_EXTENSIONS
        .iter()
        .find_map(|(mime, extension)| (*mime == mime_type).then_some(*extension))
}

fn default_download_dir() -> PathBuf {
    std::env::temp_dir().join("noloong-agent-telegram")
}

fn file_uri(path: &Path) -> Result<String, TelegramMediaResolutionError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| TelegramMediaResolutionError::Io {
                path: path.to_path_buf(),
                source,
            })?
            .join(path)
    };
    Url::from_file_path(&absolute)
        .map(|url| url.to_string())
        .map_err(|()| TelegramMediaResolutionError::InvalidFileUri { path: absolute })
}

fn telegram_metadata(
    attachment: &TelegramAttachment,
    telegram_file: &TelegramFile,
) -> serde_json::Value {
    let mut metadata = telegram_identity(attachment);
    metadata.insert("fileName".into(), json!(attachment.file.file_name));
    metadata.insert(
        "fileSize".into(),
        json!(known_file_size(attachment, telegram_file)),
    );
    metadata.insert("filePath".into(), json!(telegram_file.file_path));
    metadata.insert("width".into(), json!(attachment.width()));
    metadata.insert("height".into(), json!(attachment.height()));
    metadata.insert("duration".into(), json!(attachment.duration()));
    Value::Object(metadata)
}

fn telegram_replay_descriptor(attachment: &TelegramAttachment) -> serde_json::Value {
    let mut descriptor = telegram_identity(attachment);
    descriptor.insert("type".into(), json!("telegram_file"));
    Value::Object(descriptor)
}

fn telegram_identity(attachment: &TelegramAttachment) -> Map<String, Value> {
    Map::from_iter([
        ("fileId".into(), json!(attachment.file.file_id)),
        ("fileUniqueId".into(), json!(attachment.file.file_unique_id)),
        ("kind".into(), json!(attachment_kind_name(attachment.kind))),
    ])
}

#[cfg(test)]
mod tests {
    use super::{TelegramAttachmentResolver, TelegramMediaResolutionError};
    use crate::{
        config::TelegramFilePolicy,
        input::{TelegramAttachment, TelegramAttachmentFile, TelegramAttachmentKind},
        polling::TelegramUpdate,
        telegram_api::{
            TelegramApi, TelegramApiError, TelegramApiFuture, TelegramEditMessageTextRequest,
            TelegramFile, TelegramMessageHandle, TelegramSendMessageRequest,
            unsupported_api_future,
        },
    };
    use noloong_agent_core::{MediaKind, MediaSource};
    use std::{
        collections::BTreeMap,
        path::PathBuf,
        sync::{Arc, Mutex},
        time::{SystemTime, UNIX_EPOCH},
    };

    #[tokio::test]
    async fn resolver_inlines_small_photo_with_telegram_metadata() {
        let api = Arc::new(FakeApi::default());
        api.add_file(
            "photo-id",
            TelegramFile {
                file_id: "photo-id".into(),
                file_unique_id: Some("photo-unique".into()),
                file_size: Some(3),
                file_path: Some("photos/file.jpg".into()),
            },
        );
        api.add_download("photos/file.jpg", b"abc".to_vec());
        let resolver = resolver(api.clone(), 16, 1024, None);

        let block = resolver
            .resolve_one(&photo_attachment("photo-id", "photo-unique", Some(3)))
            .await
            .unwrap();

        assert_eq!(api.requested_file_ids(), vec!["photo-id"]);
        assert_eq!(block.kind, MediaKind::Image);
        assert_eq!(block.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(block.name.as_deref(), Some("file.jpg"));
        assert!(matches!(
            block.source,
            MediaSource::Inline { ref data, .. } if data == "YWJj"
        ));
        assert_eq!(block.metadata["telegram"]["fileId"], "photo-id");
        assert_eq!(block.metadata["telegram"]["kind"], "photo");
    }

    #[tokio::test]
    async fn resolver_writes_large_document_to_file_uri() {
        let dir = unique_test_dir("large-document");
        let _ = std::fs::remove_dir_all(&dir);
        let api = Arc::new(FakeApi::default());
        api.add_file(
            "doc-id",
            TelegramFile {
                file_id: "doc-id".into(),
                file_unique_id: Some("doc-unique".into()),
                file_size: Some(6),
                file_path: Some("documents/report.pdf".into()),
            },
        );
        api.add_download("documents/report.pdf", b"abcdef".to_vec());
        let resolver = resolver(api, 2, 1024, Some(dir.clone()));

        let block = resolver
            .resolve_one(&document_attachment(
                "doc-id",
                "doc-unique",
                Some("report.pdf"),
                Some("application/pdf"),
                Some(6),
            ))
            .await
            .unwrap();

        assert_eq!(block.kind, MediaKind::File);
        assert_eq!(block.mime_type.as_deref(), Some("application/pdf"));
        assert!(matches!(block.source, MediaSource::Uri { ref uri } if uri.starts_with("file://")));
        assert_eq!(
            std::fs::read(dir.join("doc-unique").join("report.pdf")).unwrap(),
            b"abcdef"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn resolver_rejects_oversized_file_before_download() {
        let api = Arc::new(FakeApi::default());
        api.add_file(
            "doc-id",
            TelegramFile {
                file_id: "doc-id".into(),
                file_unique_id: Some("doc-unique".into()),
                file_size: Some(20),
                file_path: Some("documents/report.pdf".into()),
            },
        );
        let resolver = resolver(api.clone(), 2, 10, None);

        let error = resolver
            .resolve_one(&document_attachment(
                "doc-id",
                "doc-unique",
                Some("report.pdf"),
                Some("application/pdf"),
                Some(20),
            ))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            TelegramMediaResolutionError::FileTooLarge {
                limit: 10,
                actual: Some(20),
                ..
            }
        ));
        assert!(api.requested_downloads().is_empty());
    }

    #[tokio::test]
    async fn resolver_rejects_missing_mime_for_document() {
        let api = Arc::new(FakeApi::default());
        api.add_file(
            "doc-id",
            TelegramFile {
                file_id: "doc-id".into(),
                file_unique_id: Some("doc-unique".into()),
                file_size: Some(3),
                file_path: Some("documents/report".into()),
            },
        );
        let resolver = resolver(api.clone(), 16, 1024, None);

        let error = resolver
            .resolve_one(&document_attachment(
                "doc-id",
                "doc-unique",
                Some("report"),
                None,
                Some(3),
            ))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            TelegramMediaResolutionError::MissingMime {
                ref file_id,
                kind: "document"
            } if file_id == "doc-id"
        ));
        assert!(api.requested_downloads().is_empty());
    }

    #[tokio::test]
    async fn resolver_reports_download_failure() {
        let api = Arc::new(FakeApi::default());
        api.add_file(
            "photo-id",
            TelegramFile {
                file_id: "photo-id".into(),
                file_unique_id: Some("photo-unique".into()),
                file_size: Some(3),
                file_path: Some("photos/file.jpg".into()),
            },
        );
        api.add_download_error(
            "photos/file.jpg",
            TelegramApiError::Network("download failed".into()),
        );
        let resolver = resolver(api, 16, 1024, None);

        let error = resolver
            .resolve_one(&photo_attachment("photo-id", "photo-unique", Some(3)))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            TelegramMediaResolutionError::Api {
                ref file_id,
                source: TelegramApiError::Network(_)
            } if file_id == "photo-id"
        ));
    }

    fn resolver(
        api: Arc<dyn TelegramApi>,
        inline_max_bytes: usize,
        max_download_bytes: usize,
        download_dir: Option<PathBuf>,
    ) -> TelegramAttachmentResolver {
        TelegramAttachmentResolver::new(
            api,
            TelegramFilePolicy {
                inline_max_bytes,
                max_download_bytes,
                download_dir,
                retention_seconds: None,
            },
        )
    }

    fn photo_attachment(
        file_id: &str,
        file_unique_id: &str,
        file_size: Option<u64>,
    ) -> TelegramAttachment {
        TelegramAttachment {
            file: TelegramAttachmentFile::new(file_id, file_unique_id, None, None, file_size),
            kind: TelegramAttachmentKind::Photo {
                width: 640,
                height: 480,
            },
        }
    }

    fn document_attachment(
        file_id: &str,
        file_unique_id: &str,
        file_name: Option<&str>,
        mime_type: Option<&str>,
        file_size: Option<u64>,
    ) -> TelegramAttachment {
        TelegramAttachment {
            file: TelegramAttachmentFile::new(
                file_id,
                file_unique_id,
                file_name,
                mime_type,
                file_size,
            ),
            kind: TelegramAttachmentKind::Document,
        }
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "noloong-telegram-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[derive(Default)]
    struct FakeApi {
        files: Mutex<BTreeMap<String, Result<TelegramFile, TelegramApiError>>>,
        downloads: Mutex<BTreeMap<String, Result<Vec<u8>, TelegramApiError>>>,
        requested_file_ids: Mutex<Vec<String>>,
        requested_downloads: Mutex<Vec<String>>,
    }

    impl FakeApi {
        fn add_file(&self, file_id: &str, file: TelegramFile) {
            self.files.lock().unwrap().insert(file_id.into(), Ok(file));
        }

        fn add_download(&self, path: &str, bytes: Vec<u8>) {
            self.downloads
                .lock()
                .unwrap()
                .insert(path.into(), Ok(bytes));
        }

        fn add_download_error(&self, path: &str, error: TelegramApiError) {
            self.downloads
                .lock()
                .unwrap()
                .insert(path.into(), Err(error));
        }

        fn requested_file_ids(&self) -> Vec<String> {
            self.requested_file_ids.lock().unwrap().clone()
        }

        fn requested_downloads(&self) -> Vec<String> {
            self.requested_downloads.lock().unwrap().clone()
        }
    }

    impl TelegramApi for FakeApi {
        fn get_updates<'a>(
            &'a self,
            _offset: Option<i64>,
            _timeout_seconds: u64,
        ) -> TelegramApiFuture<'a, Vec<TelegramUpdate>> {
            unsupported_api_future("test")
        }

        fn get_file<'a>(&'a self, file_id: &'a str) -> TelegramApiFuture<'a, TelegramFile> {
            Box::pin(async move {
                self.requested_file_ids.lock().unwrap().push(file_id.into());
                self.files
                    .lock()
                    .unwrap()
                    .get(file_id)
                    .cloned()
                    .unwrap_or_else(|| {
                        Err(TelegramApiError::Api {
                            code: 404,
                            description: "not found".into(),
                            retry_after: None,
                        })
                    })
            })
        }

        fn download_file<'a>(&'a self, file_path: &'a str) -> TelegramApiFuture<'a, Vec<u8>> {
            Box::pin(async move {
                self.requested_downloads
                    .lock()
                    .unwrap()
                    .push(file_path.into());
                self.downloads
                    .lock()
                    .unwrap()
                    .get(file_path)
                    .cloned()
                    .unwrap_or_else(|| {
                        Err(TelegramApiError::Api {
                            code: 404,
                            description: "not found".into(),
                            retry_after: None,
                        })
                    })
            })
        }

        fn send_message<'a>(
            &'a self,
            _request: TelegramSendMessageRequest,
        ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
            unsupported_api_future("test")
        }

        fn edit_message_text<'a>(
            &'a self,
            _request: TelegramEditMessageTextRequest,
        ) -> TelegramApiFuture<'a, TelegramMessageHandle> {
            unsupported_api_future("test")
        }

        fn answer_callback_query<'a>(
            &'a self,
            _callback_query_id: &'a str,
            _text: Option<&'a str>,
        ) -> TelegramApiFuture<'a, ()> {
            unsupported_api_future("test")
        }
    }
}

use crate::{
    config::WeixinFilePolicy,
    ilink_api::{
        ITEM_FILE, ITEM_IMAGE, ITEM_VIDEO, ITEM_VOICE, WeixinApi, WeixinApiError, WeixinMediaRef,
        WeixinMessageItem, build_cdn_download_url,
    },
};
use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyInit, block_padding::Pkcs7};
use base64::{Engine as _, engine::general_purpose};
use noloong_agent_core::{MediaBlock, MediaKind, MediaSource};
use serde_json::{Map, json};
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};
use thiserror::Error;
use url::Url;

type Aes128EcbEnc = ecb::Encryptor<aes::Aes128>;
type Aes128EcbDec = ecb::Decryptor<aes::Aes128>;

const WEIXIN_CDN_ALLOWLIST: &[&str] = &[
    "novac2c.cdn.weixin.qq.com",
    "ilinkai.weixin.qq.com",
    "wx.qlogo.cn",
    "thirdwx.qlogo.cn",
    "res.wx.qq.com",
    "mmbiz.qpic.cn",
    "mmbiz.qlogo.cn",
];

pub type WeixinMediaFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, WeixinMediaError>> + Send + 'a>>;

#[derive(Clone)]
pub struct WeixinAttachmentResolver {
    api: Arc<dyn WeixinApi>,
    policy: WeixinFilePolicy,
    cdn_base_url: String,
}

impl WeixinAttachmentResolver {
    pub fn new(
        api: Arc<dyn WeixinApi>,
        policy: WeixinFilePolicy,
        cdn_base_url: impl Into<String>,
    ) -> Self {
        Self {
            api,
            policy,
            cdn_base_url: cdn_base_url.into(),
        }
    }

    pub fn resolve_all<'a>(
        &'a self,
        items: &'a [WeixinMessageItem],
    ) -> WeixinMediaFuture<'a, Vec<MediaBlock>> {
        Box::pin(async move {
            let mut blocks = Vec::new();
            for item in items {
                if let Some(block) = self.resolve_one(item).await? {
                    blocks.push(block);
                }
            }
            Ok(blocks)
        })
    }

    async fn resolve_one(
        &self,
        item: &WeixinMessageItem,
    ) -> Result<Option<MediaBlock>, WeixinMediaError> {
        match item.kind {
            ITEM_IMAGE => {
                let Some(image) = &item.image_item else {
                    return Ok(None);
                };
                let Some(media) = image.media.as_ref() else {
                    return Ok(None);
                };
                let aes_key = image
                    .aeskey
                    .as_deref()
                    .map(hex_key_to_base64)
                    .transpose()?
                    .or_else(|| media.aes_key.clone());
                self.ensure_declared_download_size("image.jpg", image.mid_size)?;
                let mut block = self
                    .download_media_block(
                        media,
                        aes_key.as_deref(),
                        MediaKind::Image,
                        "image.jpg",
                        "image/jpeg",
                    )
                    .await?;
                block.metadata.insert(
                    "weixin".into(),
                    json!({"itemType": "image", "midSize": image.mid_size}),
                );
                Ok(Some(block))
            }
            ITEM_FILE => {
                let Some(file) = &item.file_item else {
                    return Ok(None);
                };
                let Some(media) = file.media.as_ref() else {
                    return Ok(None);
                };
                let file_name = file.file_name.as_deref().unwrap_or("document.bin");
                let mime_type = mime_guess::from_path(file_name)
                    .first_raw()
                    .unwrap_or("application/octet-stream");
                self.ensure_declared_download_size(
                    file_name,
                    file.len.as_deref().and_then(|len| len.parse::<u64>().ok()),
                )?;
                let mut block = self
                    .download_media_block(
                        media,
                        media.aes_key.as_deref(),
                        MediaKind::File,
                        file_name,
                        mime_type,
                    )
                    .await?;
                block.metadata.insert(
                    "weixin".into(),
                    json!({"itemType": "file", "len": file.len}),
                );
                Ok(Some(block))
            }
            ITEM_VIDEO => {
                let Some(video) = &item.video_item else {
                    return Ok(None);
                };
                let Some(media) = video.media.as_ref() else {
                    return Ok(None);
                };
                self.ensure_declared_download_size("video.mp4", video.video_size)?;
                let mut block = self
                    .download_media_block(
                        media,
                        media.aes_key.as_deref(),
                        MediaKind::File,
                        "video.mp4",
                        "video/mp4",
                    )
                    .await?;
                block.metadata.insert(
                    "weixin".into(),
                    json!({"itemType": "video", "videoSize": video.video_size}),
                );
                Ok(Some(block))
            }
            ITEM_VOICE => {
                let Some(voice) = &item.voice_item else {
                    return Ok(None);
                };
                if voice
                    .text
                    .as_deref()
                    .is_some_and(|text| !text.trim().is_empty())
                {
                    return Ok(None);
                }
                let Some(media) = voice.media.as_ref() else {
                    return Ok(None);
                };
                let mut block = self
                    .download_media_block(
                        media,
                        media.aes_key.as_deref(),
                        MediaKind::File,
                        "voice.silk",
                        "audio/silk",
                    )
                    .await?;
                block
                    .metadata
                    .insert("weixin".into(), json!({"itemType": "voice"}));
                Ok(Some(block))
            }
            _ => Ok(None),
        }
    }

    fn ensure_declared_download_size(
        &self,
        name: &str,
        declared_size: Option<u64>,
    ) -> Result<(), WeixinMediaError> {
        let Some(declared_size) = declared_size else {
            return Ok(());
        };
        if declared_size <= self.policy.max_download_bytes as u64 {
            return Ok(());
        }
        Err(WeixinMediaError::FileTooLarge {
            name: name.to_owned(),
            limit: self.policy.max_download_bytes,
            actual: declared_size.min(usize::MAX as u64) as usize,
        })
    }

    async fn download_media_block(
        &self,
        media: &WeixinMediaRef,
        aes_key: Option<&str>,
        kind: MediaKind,
        name: &str,
        mime_type: &str,
    ) -> Result<MediaBlock, WeixinMediaError> {
        let url = media_download_url(&self.cdn_base_url, media)?;
        assert_weixin_cdn_url(&url)?;
        let mut bytes = self
            .api
            .download_bytes(&url)
            .await
            .map_err(WeixinMediaError::Api)?;
        if let Some(aes_key) = aes_key.filter(|value| !value.trim().is_empty()) {
            bytes = aes128_ecb_decrypt(&bytes, &parse_aes_key(aes_key)?)?;
        }
        if bytes.len() > self.policy.max_download_bytes {
            return Err(WeixinMediaError::FileTooLarge {
                name: name.into(),
                limit: self.policy.max_download_bytes,
                actual: bytes.len(),
            });
        }
        let mut block = if bytes.len() <= self.policy.inline_max_bytes {
            MediaBlock::inline_base64(kind, general_purpose::STANDARD.encode(bytes))
        } else {
            let path = self.large_file_path(name).await?;
            tokio::fs::write(&path, &bytes)
                .await
                .map_err(|source| WeixinMediaError::Io {
                    path: path.clone(),
                    reason: source.to_string(),
                })?;
            MediaBlock::uri(kind, file_uri(&path)?)
        };
        block.mime_type = Some(mime_type.into());
        block.name = Some(name.into());
        block.replay_descriptor = Some(json!({"provider": "weixin", "name": name}));
        Ok(block)
    }

    async fn large_file_path(&self, file_name: &str) -> Result<PathBuf, WeixinMediaError> {
        let root = self
            .policy
            .download_dir
            .clone()
            .unwrap_or_else(default_download_dir);
        let dir = root.join(uuid::Uuid::new_v4().simple().to_string());
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|source| WeixinMediaError::Io {
                path: dir.clone(),
                reason: source.to_string(),
            })?;
        Ok(dir.join(sanitize_file_component(file_name)))
    }
}

pub fn aes128_ecb_encrypt(plaintext: &[u8], key: &[u8; 16]) -> Vec<u8> {
    Aes128EcbEnc::new(key.into()).encrypt_padded_vec::<Pkcs7>(plaintext)
}

pub fn aes128_ecb_decrypt(ciphertext: &[u8], key: &[u8; 16]) -> Result<Vec<u8>, WeixinMediaError> {
    Aes128EcbDec::new(key.into())
        .decrypt_padded_vec::<Pkcs7>(ciphertext)
        .map_err(|error| WeixinMediaError::Crypto(error.to_string()))
}

pub fn aes_padded_size(size: usize) -> usize {
    (size + 1).div_ceil(16) * 16
}

pub fn parse_aes_key(aes_key_b64: &str) -> Result<[u8; 16], WeixinMediaError> {
    let decoded = general_purpose::STANDARD
        .decode(aes_key_b64)
        .map_err(|error| WeixinMediaError::Crypto(format!("invalid aes_key base64: {error}")))?;
    if decoded.len() == 16 {
        return decoded.try_into().map_err(|_| {
            WeixinMediaError::Crypto("unexpected 16-byte AES key conversion failure".into())
        });
    }
    if decoded.len() == 32 {
        let text = String::from_utf8(decoded)
            .map_err(|error| WeixinMediaError::Crypto(format!("invalid hex aes_key: {error}")))?;
        let bytes = hex::decode(text.trim())
            .map_err(|error| WeixinMediaError::Crypto(format!("invalid hex aes_key: {error}")))?;
        return bytes
            .try_into()
            .map_err(|_| WeixinMediaError::Crypto("hex aes_key is not 16 bytes".into()));
    }
    Err(WeixinMediaError::Crypto(format!(
        "unexpected aes_key decoded length: {}",
        decoded.len()
    )))
}

pub fn aes_key_for_api(key: &[u8; 16]) -> String {
    general_purpose::STANDARD.encode(hex::encode(key).as_bytes())
}

pub fn media_download_url(
    cdn_base_url: &str,
    media: &WeixinMediaRef,
) -> Result<String, WeixinMediaError> {
    if let Some(param) = media
        .encrypt_query_param
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(build_cdn_download_url(cdn_base_url, param));
    }
    if let Some(url) = media
        .full_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        assert_weixin_cdn_url(url)?;
        return Ok(url.into());
    }
    Err(WeixinMediaError::MissingMediaUrl)
}

pub fn assert_weixin_cdn_url(url: &str) -> Result<(), WeixinMediaError> {
    let url = Url::parse(url).map_err(|error| WeixinMediaError::InvalidMediaUrl {
        url: url.into(),
        reason: error.to_string(),
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(WeixinMediaError::InvalidMediaUrl {
            url: url.to_string(),
            reason: "only http/https media URLs are allowed".into(),
        });
    }
    let host = url.host_str().unwrap_or_default();
    if !WEIXIN_CDN_ALLOWLIST.contains(&host) {
        return Err(WeixinMediaError::InvalidMediaUrl {
            url: url.to_string(),
            reason: format!("{host} is not in the Weixin CDN allowlist"),
        });
    }
    Ok(())
}

pub async fn media_bytes_for_outbound(media: &MediaBlock) -> Result<Vec<u8>, WeixinMediaError> {
    match &media.source {
        MediaSource::Inline { data, .. } => general_purpose::STANDARD
            .decode(data)
            .map_err(|error| WeixinMediaError::Decode(error.to_string())),
        MediaSource::Uri { uri } => {
            let path = file_uri_path(uri)?;
            tokio::fs::read(&path)
                .await
                .map_err(|source| WeixinMediaError::Io {
                    path,
                    reason: source.to_string(),
                })
        }
        MediaSource::Provider { .. } => Err(WeixinMediaError::UnsupportedMediaSource(
            "provider media cannot be sent through Weixin yet".into(),
        )),
    }
}

pub async fn ensure_outbound_file_size(
    media: &MediaBlock,
    max_bytes: usize,
) -> Result<(), WeixinMediaError> {
    let MediaSource::Uri { uri } = &media.source else {
        return Ok(());
    };
    let path = file_uri_path(uri)?;
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|source| WeixinMediaError::Io {
            path: path.clone(),
            reason: source.to_string(),
        })?;
    let actual = metadata.len().min(usize::MAX as u64) as usize;
    if actual > max_bytes {
        return Err(WeixinMediaError::FileTooLarge {
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("attachment.bin")
                .to_owned(),
            limit: max_bytes,
            actual,
        });
    }
    Ok(())
}

pub fn outbound_file_name(media: &MediaBlock) -> String {
    media
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            if let MediaSource::Uri { uri } = &media.source {
                file_uri_path(uri).ok().and_then(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_owned)
                })
            } else {
                None
            }
        })
        .unwrap_or_else(|| "attachment.bin".into())
}

pub fn outbound_mime_type(media: &MediaBlock, file_name: &str) -> String {
    media
        .mime_type
        .clone()
        .or_else(|| {
            mime_guess::from_path(file_name)
                .first_raw()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "application/octet-stream".into())
}

pub fn media_metadata(item_type: &str) -> serde_json::Value {
    let mut object = Map::new();
    object.insert("itemType".into(), json!(item_type));
    serde_json::Value::Object(object)
}

fn hex_key_to_base64(value: &str) -> Result<String, WeixinMediaError> {
    let bytes = hex::decode(value.trim())
        .map_err(|error| WeixinMediaError::Crypto(format!("invalid image aeskey hex: {error}")))?;
    Ok(general_purpose::STANDARD.encode(bytes))
}

fn file_uri_path(uri: &str) -> Result<PathBuf, WeixinMediaError> {
    let url = Url::parse(uri).map_err(|error| WeixinMediaError::InvalidMediaUrl {
        url: uri.into(),
        reason: error.to_string(),
    })?;
    if url.scheme() != "file" {
        return Err(WeixinMediaError::InvalidMediaUrl {
            url: uri.into(),
            reason: "only file:// URI media can be sent through Weixin".into(),
        });
    }
    url.to_file_path()
        .map_err(|_| WeixinMediaError::InvalidMediaUrl {
            url: uri.into(),
            reason: "file URI could not be converted to a local path".into(),
        })
}

fn file_uri(path: &Path) -> Result<String, WeixinMediaError> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| WeixinMediaError::InvalidMediaUrl {
            url: path.display().to_string(),
            reason: "path could not be converted to file URI".into(),
        })
}

fn default_download_dir() -> PathBuf {
    std::env::temp_dir().join("noloong-weixin-media")
}

fn sanitize_file_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "attachment.bin".into()
    } else {
        sanitized
    }
}

#[derive(Debug, Error)]
pub enum WeixinMediaError {
    #[error("Weixin media API failed: {0}")]
    Api(#[from] WeixinApiError),
    #[error("Weixin media file {name} is too large: limit {limit} bytes, actual {actual} bytes")]
    FileTooLarge {
        name: String,
        limit: usize,
        actual: usize,
    },
    #[error("Weixin media item has no download URL")]
    MissingMediaUrl,
    #[error("Weixin media URL is invalid: {url}: {reason}")]
    InvalidMediaUrl { url: String, reason: String },
    #[error("Weixin media crypto failed: {0}")]
    Crypto(String),
    #[error("Weixin media decode failed: {0}")]
    Decode(String),
    #[error("Weixin media I/O failed: {path}: {reason}")]
    Io { path: PathBuf, reason: String },
    #[error("Weixin media source is unsupported: {0}")]
    UnsupportedMediaSource(String),
}

#[cfg(test)]
mod tests {
    use super::{
        WeixinAttachmentResolver, aes_key_for_api, aes_padded_size, aes128_ecb_decrypt,
        aes128_ecb_encrypt, assert_weixin_cdn_url, parse_aes_key,
    };
    use crate::{
        config::WeixinFilePolicy,
        ilink_api::{
            ITEM_IMAGE, WeixinApi, WeixinApiFuture, WeixinConfigResponse, WeixinGetConfigRequest,
            WeixinGetUploadUrlRequest, WeixinImageItem, WeixinMediaRef, WeixinMessageItem,
            WeixinQrCode, WeixinQrStatus, WeixinRawResponse, WeixinSendMessageRequest,
            WeixinSendMessageResponse, WeixinSendTypingRequest, WeixinUpdatesResponse,
            WeixinUploadUrlResponse,
        },
    };
    use base64::{Engine as _, engine::general_purpose};
    use noloong_agent_core::{MediaKind, MediaSource};
    use serde_json::Map;
    use std::sync::{Arc, Mutex};

    #[test]
    fn aes_round_trips_with_pkcs7() {
        let key = [0x42; 16];
        let plaintext = b"hello world";
        let ciphertext = aes128_ecb_encrypt(plaintext, &key);

        assert_eq!(ciphertext.len(), aes_padded_size(plaintext.len()));
        assert_eq!(aes128_ecb_decrypt(&ciphertext, &key).unwrap(), plaintext);
    }

    #[test]
    fn aes_key_for_api_is_base64_encoded_hex() {
        let key = [0xab; 16];
        let encoded = aes_key_for_api(&key);

        assert_eq!(parse_aes_key(&encoded).unwrap(), key);
    }

    #[test]
    fn cdn_allowlist_rejects_non_weixin_hosts() {
        assert!(assert_weixin_cdn_url("https://novac2c.cdn.weixin.qq.com/c2c/a").is_ok());
        assert!(assert_weixin_cdn_url("https://example.com/a").is_err());
        assert!(assert_weixin_cdn_url("file:///tmp/a").is_err());
    }

    #[tokio::test]
    async fn resolver_downloads_and_decrypts_image_media() {
        let key = [0x24; 16];
        let plaintext = b"fake image bytes";
        let ciphertext = aes128_ecb_encrypt(plaintext, &key);
        let api = Arc::new(FakeMediaApi::with_download(ciphertext));
        let resolver = WeixinAttachmentResolver::new(
            api.clone(),
            WeixinFilePolicy::default(),
            "https://novac2c.cdn.weixin.qq.com/c2c",
        );
        let item = WeixinMessageItem {
            kind: ITEM_IMAGE,
            image_item: Some(WeixinImageItem {
                media: Some(WeixinMediaRef {
                    encrypt_query_param: Some("param value".into()),
                    aes_key: None,
                    encrypt_type: None,
                    full_url: None,
                    extra: Map::new(),
                }),
                aeskey: Some(hex::encode(key)),
                mid_size: Some(plaintext.len() as u64),
            }),
            ..Default::default()
        };

        let blocks = resolver.resolve_all(&[item]).await.unwrap();

        assert_eq!(blocks.len(), 1);
        let block = &blocks[0];
        assert_eq!(block.kind, MediaKind::Image);
        assert_eq!(block.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(block.name.as_deref(), Some("image.jpg"));
        match &block.source {
            MediaSource::Inline { data, .. } => {
                assert_eq!(general_purpose::STANDARD.decode(data).unwrap(), plaintext);
            }
            other => panic!("expected inline media, got {other:?}"),
        }
        let urls = api.download_urls.lock().unwrap();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].starts_with("https://novac2c.cdn.weixin.qq.com/c2c/download?"));
        assert!(urls[0].contains("encrypted_query_param=param+value"));
    }

    struct FakeMediaApi {
        download_bytes: Mutex<Vec<u8>>,
        download_urls: Mutex<Vec<String>>,
    }

    impl FakeMediaApi {
        fn with_download(download_bytes: Vec<u8>) -> Self {
            Self {
                download_bytes: Mutex::new(download_bytes),
                download_urls: Mutex::new(Vec::new()),
            }
        }
    }

    impl WeixinApi for FakeMediaApi {
        fn get_updates<'a>(
            &'a self,
            _sync_buf: &'a str,
            _timeout_ms: u64,
        ) -> WeixinApiFuture<'a, WeixinUpdatesResponse> {
            Box::pin(async { Ok(WeixinUpdatesResponse::default()) })
        }

        fn send_message<'a>(
            &'a self,
            _request: WeixinSendMessageRequest,
        ) -> WeixinApiFuture<'a, WeixinSendMessageResponse> {
            Box::pin(async { Ok(WeixinSendMessageResponse::default()) })
        }

        fn send_typing<'a>(
            &'a self,
            _request: WeixinSendTypingRequest,
        ) -> WeixinApiFuture<'a, WeixinRawResponse> {
            Box::pin(async { Ok(WeixinRawResponse::default()) })
        }

        fn get_config<'a>(
            &'a self,
            _request: WeixinGetConfigRequest,
        ) -> WeixinApiFuture<'a, WeixinConfigResponse> {
            Box::pin(async { Ok(WeixinConfigResponse::default()) })
        }

        fn get_upload_url<'a>(
            &'a self,
            _request: WeixinGetUploadUrlRequest,
        ) -> WeixinApiFuture<'a, WeixinUploadUrlResponse> {
            Box::pin(async { Ok(WeixinUploadUrlResponse::default()) })
        }

        fn upload_ciphertext<'a>(
            &'a self,
            _upload_url: &'a str,
            _ciphertext: Vec<u8>,
        ) -> WeixinApiFuture<'a, String> {
            Box::pin(async { Ok("encrypted".into()) })
        }

        fn download_bytes<'a>(&'a self, url: &'a str) -> WeixinApiFuture<'a, Vec<u8>> {
            Box::pin(async move {
                self.download_urls.lock().unwrap().push(url.into());
                Ok(self.download_bytes.lock().unwrap().clone())
            })
        }

        fn get_bot_qrcode<'a>(&'a self, _bot_type: &'a str) -> WeixinApiFuture<'a, WeixinQrCode> {
            Box::pin(async { Ok(WeixinQrCode::default()) })
        }

        fn get_qrcode_status<'a>(
            &'a self,
            _qrcode: &'a str,
        ) -> WeixinApiFuture<'a, WeixinQrStatus> {
            Box::pin(async { Ok(WeixinQrStatus::default()) })
        }
    }
}

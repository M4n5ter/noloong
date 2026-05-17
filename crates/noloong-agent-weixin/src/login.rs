use crate::{
    config::ILINK_BASE_URL,
    ilink_api::{ReqwestWeixinApi, WeixinApi, WeixinApiError, WeixinQrStatus},
    state::{WeixinAccountStore, WeixinStateError, WeixinStoredAccount, current_unix_ms},
};
use image::Luma;
use qrcode::QrCode;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeixinLoginOptions {
    pub bot_type: String,
    pub timeout_seconds: u64,
    pub qr_png_path: Option<PathBuf>,
}

impl Default for WeixinLoginOptions {
    fn default() -> Self {
        Self {
            bot_type: "3".into(),
            timeout_seconds: 480,
            qr_png_path: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeixinLoginResult {
    pub account: WeixinStoredAccount,
    pub qr_scan_data: String,
}

pub async fn run_qr_login(
    client: reqwest::Client,
    account_store: &WeixinAccountStore,
    options: WeixinLoginOptions,
    mut output: impl std::io::Write,
) -> Result<WeixinLoginResult, WeixinLoginError> {
    let api = ReqwestWeixinApi::new(client, None).with_base_url(ILINK_BASE_URL);
    let mut qr = fetch_qr(&api, &options.bot_type).await?;
    let mut qr_scan_data = render_qr_prompt(&mut output, &qr, options.qr_png_path.as_ref())?;
    output.flush()?;

    let deadline = Instant::now() + Duration::from_secs(options.timeout_seconds);
    let mut current_base_url = ILINK_BASE_URL.to_owned();
    let mut refresh_count = 0;
    while Instant::now() < deadline {
        let status_api = ReqwestWeixinApi::new(reqwest::Client::new(), None)
            .with_base_url(current_base_url.clone());
        let status = match status_api.get_qrcode_status(&qr.qrcode).await {
            Ok(status) => status,
            Err(WeixinApiError::Network(error)) => {
                log::warn!("weixin QR status polling failed: {error}");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        match status.status.as_str() {
            "wait" | "" => {
                write!(output, ".")?;
                output.flush()?;
            }
            "scaned" => {
                writeln!(output, "\n已扫码，请在微信里确认...")?;
                output.flush()?;
            }
            "scaned_but_redirect" => {
                if let Some(host) = status
                    .redirect_host
                    .as_deref()
                    .filter(|value| !value.is_empty())
                {
                    current_base_url = format!("https://{host}");
                    writeln!(output, "\n二维码服务已切换到 {host}")?;
                }
            }
            "expired" => {
                refresh_count += 1;
                if refresh_count > 3 {
                    return Err(WeixinLoginError::Expired);
                }
                writeln!(output, "\n二维码已过期，正在刷新... ({refresh_count}/3)")?;
                qr = fetch_qr(&api, &options.bot_type).await?;
                qr_scan_data = render_qr_prompt(&mut output, &qr, options.qr_png_path.as_ref())?;
                current_base_url = ILINK_BASE_URL.to_owned();
            }
            "confirmed" => {
                let account = account_from_status(status)?;
                account_store.save(&account)?;
                writeln!(output, "\n微信登录已保存：{}", account.account_id)?;
                return Ok(WeixinLoginResult {
                    account,
                    qr_scan_data,
                });
            }
            other => {
                log::debug!("weixin QR status: {other}");
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(WeixinLoginError::Timeout)
}

async fn fetch_qr(
    api: &ReqwestWeixinApi,
    bot_type: &str,
) -> Result<crate::ilink_api::WeixinQrCode, WeixinLoginError> {
    let qr = api.get_bot_qrcode(bot_type).await?;
    if qr.qrcode.trim().is_empty() {
        return Err(WeixinLoginError::Protocol(
            "QR response missing qrcode".into(),
        ));
    }
    Ok(qr)
}

fn render_qr_prompt(
    output: &mut impl std::io::Write,
    qr: &crate::ilink_api::WeixinQrCode,
    png_path: Option<&PathBuf>,
) -> Result<String, WeixinLoginError> {
    let qr_scan_data = qr
        .qrcode_img_content
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| qr.qrcode.clone());
    writeln!(output, "请使用微信扫描以下二维码：")?;
    writeln!(output, "{qr_scan_data}")?;
    let path = png_path
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir().join("noloong-weixin-login-qr.png"));
    write_qr_png(&qr_scan_data, &path)?;
    writeln!(output, "二维码图片：{}", path.display())?;
    writeln!(output, "{}", render_ascii_qr(&qr_scan_data)?)?;
    Ok(qr_scan_data)
}

pub fn write_qr_png(data: &str, path: &std::path::Path) -> Result<(), WeixinLoginError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let code = QrCode::new(data.as_bytes())
        .map_err(|error| WeixinLoginError::Protocol(format!("QR encode failed: {error}")))?;
    let image = code
        .render::<Luma<u8>>()
        .quiet_zone(true)
        .module_dimensions(12, 12)
        .build();
    image.save(path).map_err(|error| {
        WeixinLoginError::Protocol(format!("QR PNG write failed ({}): {error}", path.display()))
    })
}

fn account_from_status(status: WeixinQrStatus) -> Result<WeixinStoredAccount, WeixinLoginError> {
    let account_id = status
        .ilink_bot_id
        .or(status.ilink_user_id.clone())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            WeixinLoginError::Protocol("confirmed status missing ilink_bot_id".into())
        })?;
    let token = status
        .bot_token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| WeixinLoginError::Protocol("confirmed status missing bot_token".into()))?;
    Ok(WeixinStoredAccount {
        account_id,
        token,
        base_url: status.baseurl.unwrap_or_else(|| ILINK_BASE_URL.into()),
        user_id: status.ilink_user_id,
        saved_at_ms: current_unix_ms(),
    })
}

fn render_ascii_qr(data: &str) -> Result<String, WeixinLoginError> {
    let code = QrCode::new(data.as_bytes())
        .map_err(|error| WeixinLoginError::Protocol(format!("QR encode failed: {error}")))?;
    Ok(code
        .render::<char>()
        .dark_color(' ')
        .light_color('█')
        .quiet_zone(true)
        .module_dimensions(2, 1)
        .build())
}

#[derive(Debug, Error)]
pub enum WeixinLoginError {
    #[error("{0}")]
    Api(#[from] WeixinApiError),
    #[error("{0}")]
    State(#[from] WeixinStateError),
    #[error("Weixin QR login expired")]
    Expired,
    #[error("Weixin QR login timed out")]
    Timeout,
    #[error("Weixin QR login protocol error: {0}")]
    Protocol(String),
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::render_ascii_qr;
    use super::write_qr_png;

    #[test]
    fn ascii_qr_renders_scan_data() {
        let rendered = render_ascii_qr("https://example.com").unwrap();

        assert!(!rendered.trim().is_empty());
    }

    #[test]
    fn qr_png_is_written() {
        let path = std::env::temp_dir().join(format!(
            "noloong-weixin-login-{}.png",
            uuid::Uuid::new_v4().simple()
        ));

        write_qr_png("https://example.com", &path).unwrap();

        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }
}

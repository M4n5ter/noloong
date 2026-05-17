use base64::{Engine as _, engine::general_purpose};
use image::{ImageBuffer, ImageFormat, Rgba};
use noloong_agent_core::{AgentMessage, ContentBlock, MediaBlock, MediaKind};
use noloong_agent_weixin::{
    config::{ILINK_BASE_URL, WEIXIN_CDN_BASE_URL},
    delivery::WeixinDelivery,
    ilink_api::ReqwestWeixinApi,
    state::{SqliteWeixinStateStore, WeixinAccountStore, account_fingerprint},
};
use std::{env, io::Cursor, path::PathBuf, sync::Arc};

#[tokio::test]
#[ignore = "requires a live Weixin iLink account and sends a real image message"]
async fn live_weixin_outbound_image_smoke() {
    let account_id = required_env("WEIXIN_ACCOUNT_ID");
    let peer_id = required_env("WEIXIN_LIVE_PEER_ID");
    let stored_account = WeixinAccountStore::default_root()
        .load(&account_id)
        .expect("stored account can be loaded");
    let token = env::var("WEIXIN_TOKEN")
        .ok()
        .or_else(|| stored_account.as_ref().map(|account| account.token.clone()))
        .expect("WEIXIN_TOKEN or saved Weixin login token is required");
    let base_url = env::var("WEIXIN_BASE_URL")
        .ok()
        .or_else(|| {
            stored_account
                .as_ref()
                .map(|account| account.base_url.clone())
        })
        .unwrap_or_else(|| ILINK_BASE_URL.into());
    let cdn_base_url =
        env::var("WEIXIN_CDN_BASE_URL").unwrap_or_else(|_| WEIXIN_CDN_BASE_URL.into());
    let state_database_url =
        env::var("NOLOONG_STATE_DATABASE_URL").unwrap_or_else(|_| default_state_database_url());
    let state = Arc::new(
        SqliteWeixinStateStore::new(state_database_url, account_fingerprint(&account_id)).unwrap(),
    );
    let api = Arc::new(
        ReqwestWeixinApi::new(reqwest::Client::new(), Some(token))
            .with_base_url(base_url)
            .with_cdn_base_url(cdn_base_url.clone()),
    );
    let delivery = WeixinDelivery::new(api, state.clone(), cdn_base_url, 2000, 1024 * 1024);
    let mut media = MediaBlock::inline_base64(
        MediaKind::Image,
        general_purpose::STANDARD.encode(visible_png_bytes()),
    );
    media.mime_type = Some("image/png".into());
    media.name = Some("noloong-weixin-live-smoke.png".into());
    let message = AgentMessage::assistant(
        "live-outbound-media-smoke",
        vec![
            ContentBlock::Text {
                text: "noloong weixin outbound media smoke".into(),
            },
            ContentBlock::Media { media },
        ],
    );

    delivery
        .send_agent_message(&peer_id, &message)
        .await
        .expect("live Weixin outbound image should be delivered");
}

fn visible_png_bytes() -> Vec<u8> {
    let mut image = ImageBuffer::from_pixel(128, 128, Rgba([238_u8, 68, 68, 255]));
    for x in 0..128 {
        image.put_pixel(x, x, Rgba([255, 255, 255, 255]));
        image.put_pixel(127 - x, x, Rgba([32, 32, 32, 255]));
    }
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("test image can be encoded as png");
    bytes.into_inner()
}

fn required_env(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required for this ignored live test"))
}

fn default_state_database_url() -> String {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    format!(
        "sqlite://{}",
        home.join(".agents")
            .join("noloong")
            .join("state.sqlite")
            .display()
    )
}

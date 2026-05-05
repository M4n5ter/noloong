use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

pub fn unsigned_jwt(payload: serde_json::Value) -> String {
    format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
        URL_SAFE_NO_PAD.encode(payload.to_string()),
        ""
    )
}

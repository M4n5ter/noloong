#[path = "support/jwt.rs"]
mod jwt_support;

use jwt_support::unsigned_jwt;
use noloong_openai::auth::{ChatGptTokenClaims, ChatGptTokenData};
use serde_json::json;

#[test]
fn auth_token_parses_chatgpt_id_token_claims() -> noloong_openai::Result<()> {
    let jwt = unsigned_jwt(json!({
        "email": "user@example.test",
        "https://api.openai.com/auth/plan_type": "plus",
        "https://api.openai.com/auth/user_id": "user-123",
        "https://api.openai.com/auth/account_id": "account-123",
        "https://api.openai.com/auth/fedramp": true,
        "exp": 1234567890_u64
    }));

    let claims = ChatGptTokenClaims::from_jwt(&jwt)?;

    assert_eq!(claims.email.as_deref(), Some("user@example.test"));
    assert_eq!(claims.plan_type.as_deref(), Some("plus"));
    assert_eq!(claims.chatgpt_user_id.as_deref(), Some("user-123"));
    assert_eq!(claims.account_id.as_deref(), Some("account-123"));
    assert!(claims.fedramp);
    assert_eq!(claims.exp, Some(1234567890));
    Ok(())
}

#[test]
fn auth_token_debug_redacts_secret_fields() {
    let token = ChatGptTokenData::new("id-secret", "access-secret", "refresh-secret", 42)
        .account_id("account-123");
    let debug = format!("{token:?}");

    assert!(debug.contains("<redacted>"));
    assert!(debug.contains("account-123"));
    assert!(!debug.contains("id-secret"));
    assert!(!debug.contains("access-secret"));
    assert!(!debug.contains("refresh-secret"));
}

#[test]
fn auth_token_parses_codex_nested_chatgpt_claims() -> noloong_openai::Result<()> {
    let jwt = unsigned_jwt(json!({
        "https://api.openai.com/profile": {
            "email": "profile@example.test"
        },
        "https://api.openai.com/auth": {
            "chatgpt_plan_type": "pro",
            "chatgpt_user_id": "user-nested",
            "chatgpt_account_id": "account-nested",
            "chatgpt_account_is_fedramp": true
        }
    }));

    let claims = ChatGptTokenClaims::from_jwt(&jwt)?;

    assert_eq!(claims.email.as_deref(), Some("profile@example.test"));
    assert_eq!(claims.plan_type.as_deref(), Some("pro"));
    assert_eq!(claims.chatgpt_user_id.as_deref(), Some("user-nested"));
    assert_eq!(claims.account_id.as_deref(), Some("account-nested"));
    assert!(claims.fedramp);
    Ok(())
}

#[test]
fn auth_token_invalid_jwt_reports_parse_error() {
    let error =
        ChatGptTokenClaims::from_jwt("not-a-jwt").expect_err("invalid jwt should be rejected");

    assert!(error.to_string().contains("JWT"));
}

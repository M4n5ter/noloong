//! Browser and device-code login primitives for ChatGPT authentication.

mod browser;
mod config;
mod device;
mod exchange;
mod pkce;

pub use browser::{
    BrowserCallback, BrowserLoginServer, BrowserLoginSession, complete_browser_login,
};
pub use config::{
    ChatGptLoginConfig, DEFAULT_BROWSER_CALLBACK_PORT, DEFAULT_CLIENT_ID, DEFAULT_ISSUER,
    DEFAULT_SCOPE, FALLBACK_BROWSER_CALLBACK_PORT,
};
pub use device::{
    ChatGptDeviceCode, DeviceAuthorization, DeviceAuthorizationPoll, DeviceAuthorizationStatus,
    complete_device_authorization, poll_device_authorization, request_device_authorization,
};
pub use exchange::{
    ExchangedTokens, exchange_authorization_code, persist_exchanged_tokens,
    token_data_from_exchange,
};
pub use pkce::{PkceCodes, code_challenge_for_verifier, generate_pkce, generate_state};

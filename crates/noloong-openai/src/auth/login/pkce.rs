use crate::Result;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PkceCodes {
    pub code_verifier: String,
    pub code_challenge: String,
}

impl PkceCodes {
    pub fn from_verifier(code_verifier: impl Into<String>) -> Self {
        let code_verifier = code_verifier.into();
        let code_challenge = code_challenge_for_verifier(&code_verifier);
        Self {
            code_verifier,
            code_challenge,
        }
    }
}

pub fn generate_pkce() -> Result<PkceCodes> {
    let code_verifier = random_url_safe(64)?;
    Ok(PkceCodes::from_verifier(code_verifier))
}

pub fn generate_state() -> Result<String> {
    random_url_safe(32)
}

pub fn code_challenge_for_verifier(code_verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()))
}

fn random_url_safe(len: usize) -> Result<String> {
    let mut bytes = vec![0; len];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

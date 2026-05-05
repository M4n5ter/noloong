use noloong_openai::auth::{
    ChatGptFileTokenStorage, ChatGptLoginConfig, complete_device_authorization,
    request_device_authorization,
};
use std::{env, path::PathBuf};

const TOKEN_FILE_ENV: &str = "NOLOONG_CHATGPT_TOKEN_FILE";

#[tokio::main]
async fn main() -> noloong_openai::Result<()> {
    let token_path = token_path()?;
    let client = reqwest::Client::new();
    let config = ChatGptLoginConfig::new();
    let device = request_device_authorization(&client, &config).await?;

    eprintln!("Open this URL in a browser:");
    eprintln!("{}", device.verification_url);
    eprintln!();
    eprintln!("Enter this code:");
    eprintln!("{}", device.user_code);
    eprintln!();
    eprintln!("Waiting for browser authorization...");

    let storage = ChatGptFileTokenStorage::new(&token_path);
    let token = complete_device_authorization(&client, &config, device, &storage).await?;
    let account = token.account_id.as_deref().unwrap_or("<unknown>");
    eprintln!("Saved ChatGPT token to {}", token_path.display());
    eprintln!("Account: {account}");
    Ok(())
}

fn token_path() -> noloong_openai::Result<PathBuf> {
    let Some(path) = env::args_os()
        .nth(1)
        .or_else(|| env::var_os(TOKEN_FILE_ENV))
    else {
        return Err(noloong_openai::OpenAiIntegrationError::Login(format!(
            "usage: chatgpt_device_login <token-file> or set {TOKEN_FILE_ENV}"
        )));
    };
    Ok(PathBuf::from(path))
}

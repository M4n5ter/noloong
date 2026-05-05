use noloong_openai::auth::{
    BrowserLoginServer, ChatGptFileTokenStorage, ChatGptLoginConfig, complete_browser_login,
};
use std::{env, path::PathBuf};

const TOKEN_FILE_ENV: &str = "NOLOONG_CHATGPT_TOKEN_FILE";

#[tokio::main]
async fn main() -> noloong_openai::Result<()> {
    let token_path = token_path()?;
    let client = reqwest::Client::new();
    let config = ChatGptLoginConfig::new();
    let server = BrowserLoginServer::bind(config.clone()).await?;

    eprintln!("Open this URL in a browser:");
    eprintln!("{}", server.authorization_url());
    eprintln!();
    eprintln!("Waiting for browser redirect on localhost...");

    let session = server.session().clone();
    let callback = server.wait_for_callback().await?;
    let storage = ChatGptFileTokenStorage::new(&token_path);
    let token = complete_browser_login(&client, &config, &session, callback, &storage).await?;
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
            "usage: chatgpt_browser_login <token-file> or set {TOKEN_FILE_ENV}"
        )));
    };
    Ok(PathBuf::from(path))
}

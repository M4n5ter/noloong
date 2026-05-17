use crate::config::resolve_chatgpt_token_file;
use clap::{Args, Subcommand, ValueEnum};
use noloong_openai::{
    OpenAiIntegrationError,
    auth::{
        BrowserLoginServer, ChatGptLoginConfig, ChatGptTokenData, ChatGptTokenStorage,
        ChatGptTokenStore, complete_browser_login, complete_device_authorization,
        request_device_authorization,
    },
};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub struct ChatGptOptions {
    #[command(subcommand)]
    pub command: ChatGptCommand,
}

#[derive(Clone, Debug, Subcommand, PartialEq, Eq)]
pub enum ChatGptCommand {
    Login(ChatGptLoginOptions),
    Status(ChatGptTokenFileOptions),
    Logout(ChatGptTokenFileOptions),
}

#[derive(Clone, Debug, Args, PartialEq, Eq)]
pub struct ChatGptLoginOptions {
    #[arg(long = "flow", value_enum, default_value_t = ChatGptLoginFlow::Browser)]
    pub flow: ChatGptLoginFlow,
    #[command(flatten)]
    pub token_file: ChatGptTokenFileOptions,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum, PartialEq, Eq)]
pub enum ChatGptLoginFlow {
    #[default]
    Browser,
    Device,
}

#[derive(Clone, Debug, Default, Args, PartialEq, Eq)]
pub struct ChatGptTokenFileOptions {
    #[arg(long = "token-file")]
    pub token_file: Option<String>,
}

pub async fn run_chatgpt(options: ChatGptOptions) -> Result<(), ChatGptCliError> {
    match options.command {
        ChatGptCommand::Login(options) => login(options).await,
        ChatGptCommand::Status(options) => status(options),
        ChatGptCommand::Logout(options) => logout(options),
    }
}

async fn login(options: ChatGptLoginOptions) -> Result<(), ChatGptCliError> {
    let token_file = resolve_token_file(&options.token_file)?;
    match options.flow {
        ChatGptLoginFlow::Browser => browser_login(token_file).await,
        ChatGptLoginFlow::Device => device_login(token_file).await,
    }
}

async fn browser_login(token_file: PathBuf) -> Result<(), ChatGptCliError> {
    let config = ChatGptLoginConfig::new();
    let storage = token_storage(&token_file);
    let client = reqwest::Client::new();
    let server = BrowserLoginServer::bind(config.clone()).await?;
    let session = server.session().clone();
    println!("Open this URL in your browser:");
    println!("{}", server.authorization_url());
    println!("Waiting for browser login callback...");
    let callback = server.wait_for_callback().await?;
    let token = complete_browser_login(&client, &config, &session, callback, &storage).await?;
    println!("{}", login_success_message(&token_file, &token));
    Ok(())
}

async fn device_login(token_file: PathBuf) -> Result<(), ChatGptCliError> {
    let config = ChatGptLoginConfig::new();
    let storage = token_storage(&token_file);
    let client = reqwest::Client::new();
    let device_code = request_device_authorization(&client, &config).await?;
    println!("Open this URL in your browser:");
    println!("{}", device_code.verification_url);
    println!("Enter this code:");
    println!("{}", device_code.user_code);
    println!("Waiting for device authorization...");
    let token = complete_device_authorization(&client, &config, device_code, &storage).await?;
    println!("{}", login_success_message(&token_file, &token));
    Ok(())
}

fn status(options: ChatGptTokenFileOptions) -> Result<(), ChatGptCliError> {
    let token_file = resolve_token_file(&options)?;
    let storage = token_storage(&token_file);
    let message = match storage.load()? {
        Some(token) => token_status_message(&token_file, &token),
        None => format!(
            "ChatGPT is not logged in.\nToken file: {}\nRun: noloong chatgpt login --flow browser",
            token_file.display()
        ),
    };
    println!("{message}");
    Ok(())
}

fn logout(options: ChatGptTokenFileOptions) -> Result<(), ChatGptCliError> {
    let token_file = resolve_token_file(&options)?;
    let storage = token_storage(&token_file);
    storage.delete()?;
    println!("ChatGPT token file removed: {}", token_file.display());
    Ok(())
}

fn resolve_token_file(options: &ChatGptTokenFileOptions) -> Result<PathBuf, ChatGptCliError> {
    resolve_chatgpt_token_file(options.token_file.as_deref(), None).map_err(Into::into)
}

fn token_storage(path: &Path) -> ChatGptTokenStorage {
    ChatGptTokenStorage::file(path.to_path_buf())
}

fn login_success_message(path: &Path, token: &ChatGptTokenData) -> String {
    format!(
        "ChatGPT login completed.\nToken file: {}\n{}",
        path.display(),
        token_summary(token)
    )
}

fn token_status_message(path: &Path, token: &ChatGptTokenData) -> String {
    format!(
        "ChatGPT is logged in.\nToken file: {}\n{}",
        path.display(),
        token_summary(token)
    )
}

fn token_summary(token: &ChatGptTokenData) -> String {
    match token.id_token_claims() {
        Ok(claims) => {
            let mut lines = Vec::new();
            if let Some(email) = claims.email {
                lines.push(format!("Email: {email}"));
            }
            if let Some(plan) = claims.plan_type {
                lines.push(format!("Plan: {plan}"));
            }
            if let Some(account_id) = token.account_id.as_ref().or(claims.account_id.as_ref()) {
                lines.push(format!("Account: {account_id}"));
            }
            if claims.fedramp {
                lines.push("FedRAMP: true".into());
            }
            if let Some(exp) = claims.exp {
                lines.push(format!("Token expires at: {exp}"));
            }
            if lines.is_empty() {
                "Token claims: unavailable".into()
            } else {
                lines.join("\n")
            }
        }
        Err(error) => format!("Token claims: unavailable ({error})"),
    }
}

#[derive(Debug, Error)]
pub enum ChatGptCliError {
    #[error("{0}")]
    Config(#[from] crate::config::CliConfigError),
    #[error("{0}")]
    OpenAi(#[from] OpenAiIntegrationError),
}

#[cfg(test)]
mod tests {
    use super::{ChatGptLoginFlow, ChatGptTokenFileOptions, token_status_message};
    use crate::cli::{Cli, CliCommand};
    use clap::Parser;
    use noloong_openai::auth::ChatGptTokenData;
    use std::path::Path;

    #[test]
    fn cli_chatgpt_login_defaults_to_browser_flow() {
        let cli = Cli::try_parse_from(["noloong", "chatgpt", "login"]).unwrap();

        let CliCommand::ChatGpt(options) = cli.command else {
            panic!("expected chatgpt command");
        };
        let super::ChatGptCommand::Login(login) = options.command else {
            panic!("expected login command");
        };
        assert_eq!(login.flow, ChatGptLoginFlow::Browser);
        assert_eq!(login.token_file, ChatGptTokenFileOptions::default());
    }

    #[test]
    fn cli_chatgpt_login_accepts_device_flow_and_token_file() {
        let cli = Cli::try_parse_from([
            "noloong",
            "chatgpt",
            "login",
            "--flow",
            "device",
            "--token-file",
            "/tmp/token.json",
        ])
        .unwrap();

        let CliCommand::ChatGpt(options) = cli.command else {
            panic!("expected chatgpt command");
        };
        let super::ChatGptCommand::Login(login) = options.command else {
            panic!("expected login command");
        };
        assert_eq!(login.flow, ChatGptLoginFlow::Device);
        assert_eq!(
            login.token_file.token_file.as_deref(),
            Some("/tmp/token.json")
        );
    }

    #[test]
    fn token_status_message_does_not_expose_token_secrets() {
        let token = ChatGptTokenData::new("id-secret", "access-secret", "refresh-secret", 42)
            .account_id("account-123");

        let message = token_status_message(Path::new("/tmp/token.json"), &token);

        assert!(!message.contains("id-secret"));
        assert!(!message.contains("access-secret"));
        assert!(!message.contains("refresh-secret"));
        assert!(message.contains("/tmp/token.json"));
    }
}

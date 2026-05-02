use noloong_agent_core::{AgentRuntime, StdioExtensionConfig};
use std::{path::PathBuf, time::Duration};

#[tokio::main]
async fn main() -> noloong_agent_core::Result<()> {
    let extension = workspace_root()
        .join("examples")
        .join("extensions")
        .join("ai-sdk-provider")
        .join("stdio-ai-sdk-extension.ts");

    let runtime = AgentRuntime::builder()
        .with_stdio_extension(
            StdioExtensionConfig::new("npx")
                .args(["tsx".to_string(), extension.to_string_lossy().to_string()])
                .request_timeout(Duration::from_secs(60)),
        )
        .await?
        .build()?;

    let report = runtime.run("Say hello from the TS AI SDK provider").await?;
    println!("messages: {}", report.state.messages.len());
    Ok(())
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is inside crates/noloong-agent-core")
        .to_path_buf()
}

use noloong_agent::{HostProcessManager, ReadOutputRequest, StartCommandRequest};
use std::{collections::BTreeMap, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manager = HostProcessManager::new();

    let fast = manager
        .start(StartCommandRequest {
            command: "printf fast".into(),
            shell: Some("sh".into()),
            cwd: Some(PathBuf::from(".")),
            env: BTreeMap::new(),
            pipe_stdin: false,
            max_spool_bytes: None,
            foreground_wait_ms: Some(1000),
        })
        .await?;
    println!("fast job: {} {:?}", fast.job_id, fast.status);

    let slow = manager
        .start(StartCommandRequest {
            command: "sleep 1; printf slow".into(),
            shell: Some("sh".into()),
            cwd: Some(PathBuf::from(".")),
            env: BTreeMap::new(),
            pipe_stdin: false,
            max_spool_bytes: None,
            foreground_wait_ms: Some(10),
        })
        .await?;
    println!("slow job started in background: {}", slow.job_id);

    let wait = manager.wait(&slow.job_id, Some(3000)).await?;
    println!("slow job wait: {:?}", wait.status);

    let output = manager
        .read(
            &slow.job_id,
            ReadOutputRequest {
                after_seq: Some(0),
                max_bytes: None,
                wait_ms: Some(100),
            },
        )
        .await?;
    let text = output
        .chunks
        .iter()
        .map(|chunk| chunk.text.as_str())
        .collect::<String>();
    println!("slow job output: {text}");

    manager.close().await?;
    Ok(())
}

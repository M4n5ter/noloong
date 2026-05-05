use noloong_agent::{
    HostProcessEvent, HostProcessManager, HostProcessSubscription, JobStatus, ProcessOutputStream,
    ReadOutputRequest, StartCommandRequest,
};
use std::{collections::BTreeMap, path::PathBuf};
use tokio::{
    sync::mpsc,
    time::{Duration, timeout},
};

#[tokio::test]
async fn host_process_manager_start_returns_completed_when_fast() {
    let manager = HostProcessManager::new();
    let snapshot = manager
        .start(StartCommandRequest {
            command: "printf fast".into(),
            foreground_wait_ms: Some(1000),
            ..start_defaults()
        })
        .await
        .unwrap();

    assert!(matches!(
        snapshot.status,
        JobStatus::Exited { code: Some(0) }
    ));
    let output = manager
        .read(
            &snapshot.job_id,
            ReadOutputRequest {
                after_seq: None,
                max_bytes: None,
                wait_ms: Some(100),
            },
        )
        .await
        .unwrap();
    assert_eq!(joined_output(&output.chunks), "fast");
}

#[tokio::test]
async fn host_process_manager_start_returns_running_when_slow() {
    let manager = HostProcessManager::new();
    let snapshot = manager
        .start(StartCommandRequest {
            command: "sleep 1; printf slow".into(),
            foreground_wait_ms: Some(10),
            ..start_defaults()
        })
        .await
        .unwrap();

    assert!(matches!(snapshot.status, JobStatus::Running));
    let outcome = manager.wait(&snapshot.job_id, Some(3000)).await.unwrap();
    assert!(!outcome.timed_out);
    assert!(matches!(
        outcome.status,
        JobStatus::Exited { code: Some(0) }
    ));
}

#[tokio::test]
async fn host_process_manager_read_wait_list() {
    let manager = HostProcessManager::new();
    let snapshot = manager
        .start(StartCommandRequest {
            command: "printf hello".into(),
            foreground_wait_ms: Some(1000),
            ..start_defaults()
        })
        .await
        .unwrap();

    let jobs = manager.list().await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].job_id, snapshot.job_id);

    let output = manager
        .read(
            &snapshot.job_id,
            ReadOutputRequest {
                after_seq: Some(0),
                max_bytes: Some(1024),
                wait_ms: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(joined_output(&output.chunks), "hello");
}

#[tokio::test]
async fn host_process_manager_interactive_write() {
    let manager = HostProcessManager::new();
    let snapshot = manager
        .start(StartCommandRequest {
            command: "read line; printf \"echo:%s\" \"$line\"".into(),
            pipe_stdin: true,
            foreground_wait_ms: Some(10),
            ..start_defaults()
        })
        .await
        .unwrap();

    assert!(matches!(snapshot.status, JobStatus::Running));
    manager.write(&snapshot.job_id, "hello\n").await.unwrap();
    manager.wait(&snapshot.job_id, Some(3000)).await.unwrap();
    let output = manager
        .read(
            &snapshot.job_id,
            ReadOutputRequest {
                after_seq: None,
                max_bytes: None,
                wait_ms: Some(100),
            },
        )
        .await
        .unwrap();
    assert_eq!(joined_output(&output.chunks), "echo:hello");
}

#[tokio::test]
async fn host_process_manager_wait_timeout_does_not_kill() {
    let manager = HostProcessManager::new();
    let snapshot = manager
        .start(StartCommandRequest {
            command: "sleep 1; printf done".into(),
            foreground_wait_ms: Some(10),
            ..start_defaults()
        })
        .await
        .unwrap();

    let outcome = manager.wait(&snapshot.job_id, Some(10)).await.unwrap();
    assert!(outcome.timed_out);
    assert!(matches!(outcome.status, JobStatus::Running));

    let outcome = manager.wait(&snapshot.job_id, Some(3000)).await.unwrap();
    assert!(!outcome.timed_out);
    assert!(matches!(
        outcome.status,
        JobStatus::Exited { code: Some(0) }
    ));
}

#[tokio::test]
async fn host_process_output_cursor_order() {
    let manager = HostProcessManager::new();
    let snapshot = manager
        .start(StartCommandRequest {
            command: "printf first; printf second >&2".into(),
            foreground_wait_ms: Some(1000),
            ..start_defaults()
        })
        .await
        .unwrap();
    let output = manager
        .read(
            &snapshot.job_id,
            ReadOutputRequest {
                after_seq: Some(0),
                max_bytes: None,
                wait_ms: Some(100),
            },
        )
        .await
        .unwrap();

    assert!(
        output
            .chunks
            .windows(2)
            .all(|pair| pair[0].seq < pair[1].seq)
    );
    assert!(
        output
            .chunks
            .iter()
            .any(|chunk| chunk.stream == ProcessOutputStream::Stdout)
    );
    assert!(
        output
            .chunks
            .iter()
            .any(|chunk| chunk.stream == ProcessOutputStream::Stderr)
    );
}

#[tokio::test]
async fn host_process_output_cap_and_cursor() {
    let manager = HostProcessManager::new();
    let snapshot = manager
        .start(StartCommandRequest {
            command: "printf first; sleep 0.05; printf second; sleep 0.05; printf third".into(),
            foreground_wait_ms: Some(1000),
            ..start_defaults()
        })
        .await
        .unwrap();
    let first = manager
        .read(
            &snapshot.job_id,
            ReadOutputRequest {
                after_seq: Some(0),
                max_bytes: Some(6),
                wait_ms: Some(100),
            },
        )
        .await
        .unwrap();
    let second = manager
        .read(
            &snapshot.job_id,
            ReadOutputRequest {
                after_seq: Some(first.next_cursor),
                max_bytes: None,
                wait_ms: Some(100),
            },
        )
        .await
        .unwrap();

    assert!(!first.chunks.is_empty());
    assert!(first.truncated);
    assert!(second.next_cursor >= first.next_cursor);
}

#[tokio::test]
async fn host_process_manager_session_cleanup() {
    let manager = HostProcessManager::new();
    let snapshot = manager
        .start(StartCommandRequest {
            command: "sleep 5".into(),
            foreground_wait_ms: Some(10),
            ..start_defaults()
        })
        .await
        .unwrap();

    manager.close().await.unwrap();
    let jobs = manager.list().await.unwrap();
    let job = jobs
        .iter()
        .find(|job| job.job_id == snapshot.job_id)
        .expect("job should remain listed after cleanup");
    assert!(matches!(job.status, JobStatus::Terminated));
}

#[tokio::test]
async fn host_process_completion_event_exited() {
    let manager = HostProcessManager::new();
    let mut events = subscribe_events(&manager);
    let snapshot = manager
        .start(StartCommandRequest {
            command: "printf done".into(),
            foreground_wait_ms: Some(1000),
            ..start_defaults()
        })
        .await
        .unwrap();

    let completion = events.next_completion().await;

    assert_eq!(completion.snapshot.job_id, snapshot.job_id);
    assert!(matches!(
        completion.snapshot.status,
        JobStatus::Exited { code: Some(0) }
    ));
    assert_eq!(joined_output(&completion.output.chunks), "done");
}

#[tokio::test]
async fn host_process_completion_event_terminated() {
    let manager = HostProcessManager::new();
    let mut events = subscribe_events(&manager);
    let snapshot = manager
        .start(StartCommandRequest {
            command: "sleep 5".into(),
            foreground_wait_ms: Some(10),
            ..start_defaults()
        })
        .await
        .unwrap();

    manager.terminate(&snapshot.job_id).await.unwrap();
    let completion = events.next_completion().await;

    assert_eq!(completion.snapshot.job_id, snapshot.job_id);
    assert!(matches!(completion.snapshot.status, JobStatus::Terminated));
}

#[tokio::test]
async fn host_process_completion_event_is_single_delivery() {
    let manager = HostProcessManager::new();
    let mut events = subscribe_events(&manager);
    let snapshot = manager
        .start(StartCommandRequest {
            command: "printf once".into(),
            foreground_wait_ms: Some(1000),
            ..start_defaults()
        })
        .await
        .unwrap();

    let completion = events.next_completion().await;
    assert_eq!(completion.snapshot.job_id, snapshot.job_id);
    manager.wait(&snapshot.job_id, Some(100)).await.unwrap();
    let _ = manager.list().await.unwrap();
    let _ = manager
        .read(
            &snapshot.job_id,
            ReadOutputRequest {
                after_seq: Some(0),
                max_bytes: None,
                wait_ms: Some(10),
            },
        )
        .await
        .unwrap();

    assert!(events.receiver.try_recv().is_err());
}

fn start_defaults() -> StartCommandRequest {
    StartCommandRequest {
        command: String::new(),
        shell: Some("sh".into()),
        cwd: Some(PathBuf::from(".")),
        env: BTreeMap::new(),
        pipe_stdin: false,
        max_spool_bytes: None,
        foreground_wait_ms: None,
    }
}

fn joined_output(chunks: &[noloong_agent::OutputChunk]) -> String {
    chunks.iter().map(|chunk| chunk.text.as_str()).collect()
}

struct EventProbe {
    _subscription: HostProcessSubscription,
    receiver: mpsc::UnboundedReceiver<HostProcessEvent>,
}

impl EventProbe {
    async fn next_completion(&mut self) -> noloong_agent::HostProcessCompletion {
        let event = timeout(Duration::from_secs(2), self.receiver.recv())
            .await
            .expect("completion event arrives before timeout")
            .expect("completion event channel remains open");
        match event {
            HostProcessEvent::JobCompleted { completion } => completion,
        }
    }
}

fn subscribe_events(manager: &HostProcessManager) -> EventProbe {
    let (sender, receiver) = mpsc::unbounded_channel();
    let subscription = manager.subscribe(move |event| {
        let _ = sender.send(event);
    });
    EventProbe {
        _subscription: subscription,
        receiver,
    }
}

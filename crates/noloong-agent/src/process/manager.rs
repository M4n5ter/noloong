use crate::{host::default_shell, text};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, Notify, mpsc},
    task::JoinHandle,
    time::{Duration, timeout},
};

pub type JobId = String;
pub(crate) const PROCESS_EMPTY_COMMAND_MESSAGE: &str = "command must not be empty";
pub(crate) const PROCESS_STDIN_DISABLED_PREFIX: &str = "job ";
pub(crate) const PROCESS_STDIN_DISABLED_SUFFIX: &str = " does not accept stdin";
const DEFAULT_COMPLETION_TAIL_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartCommandRequest {
    pub command: String,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub pipe_stdin: bool,
    #[serde(default)]
    pub max_spool_bytes: Option<usize>,
    #[serde(default)]
    pub foreground_wait_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadOutputRequest {
    #[serde(default)]
    pub after_seq: Option<u64>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessOutput {
    pub job_id: JobId,
    pub chunks: Vec<OutputChunk>,
    pub next_cursor: u64,
    pub dropped_before_seq: u64,
    pub truncated: bool,
    pub status: JobStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutputChunk {
    pub seq: u64,
    pub stream: ProcessOutputStream,
    pub text: String,
    pub byte_len: usize,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessOutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Exited { code: Option<i32> },
    Terminated,
    Failed { error: String },
}

impl JobStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub job_id: JobId,
    pub command: String,
    pub shell: String,
    pub cwd: PathBuf,
    pub status: JobStatus,
    pub started_at_ms: u64,
    #[serde(default)]
    pub ended_at_ms: Option<u64>,
    pub next_cursor: u64,
    pub dropped_before_seq: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WaitOutcome {
    pub job_id: JobId,
    pub status: JobStatus,
    pub timed_out: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostProcessCompletion {
    pub snapshot: JobSnapshot,
    pub output: ProcessOutput,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostProcessEvent {
    JobCompleted { completion: HostProcessCompletion },
}

#[derive(Clone, Debug)]
pub struct HostProcessManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    counter: AtomicU64,
    listener_counter: AtomicU64,
    jobs: Mutex<BTreeMap<JobId, Arc<JobHandle>>>,
    listeners: StdMutex<BTreeMap<u64, HostProcessListener>>,
    default_spool_bytes: usize,
}

type HostProcessListener = Arc<dyn Fn(HostProcessEvent) + Send + Sync + 'static>;

impl std::fmt::Debug for ManagerInner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagerInner")
            .field("default_spool_bytes", &self.default_spool_bytes)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct HostProcessSubscription {
    inner: Arc<ManagerInner>,
    id: u64,
}

impl Drop for HostProcessSubscription {
    fn drop(&mut self) {
        self.inner
            .listeners
            .lock()
            .expect("host process listeners lock poisoned")
            .remove(&self.id);
    }
}

#[derive(Debug)]
struct JobHandle {
    job_id: JobId,
    command: String,
    shell: String,
    cwd: PathBuf,
    control: mpsc::UnboundedSender<JobControl>,
    stdin: Mutex<Option<ChildStdin>>,
    status: Mutex<JobStatus>,
    ended_at_ms: Mutex<Option<u64>>,
    output: Mutex<OutputBuffer>,
    notify: Notify,
    started_at_ms: u64,
}

#[derive(Debug)]
struct OutputBuffer {
    chunks: VecDeque<OutputChunk>,
    next_seq: u64,
    dropped_before_seq: u64,
    total_bytes: usize,
    max_bytes: usize,
}

#[derive(Debug)]
enum JobControl {
    Terminate,
}

impl Default for HostProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HostProcessManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ManagerInner {
                counter: AtomicU64::new(0),
                listener_counter: AtomicU64::new(0),
                jobs: Mutex::new(BTreeMap::new()),
                listeners: StdMutex::new(BTreeMap::new()),
                default_spool_bytes: 1024 * 1024,
            }),
        }
    }

    pub fn subscribe(
        &self,
        listener: impl Fn(HostProcessEvent) + Send + Sync + 'static,
    ) -> HostProcessSubscription {
        let id = self.inner.listener_counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.inner
            .listeners
            .lock()
            .expect("host process listeners lock poisoned")
            .insert(id, Arc::new(listener));
        HostProcessSubscription {
            inner: Arc::clone(&self.inner),
            id,
        }
    }

    pub async fn start(&self, request: StartCommandRequest) -> Result<JobSnapshot, ProcessError> {
        if request.command.trim().is_empty() {
            return Err(ProcessError::Invalid(PROCESS_EMPTY_COMMAND_MESSAGE.into()));
        }
        let job_id = format!(
            "host-job-{}",
            self.inner.counter.fetch_add(1, Ordering::SeqCst) + 1
        );
        let shell = request.shell.unwrap_or_else(default_shell);
        let cwd = match request.cwd {
            Some(cwd) => cwd,
            None => std::env::current_dir().map_err(|error| ProcessError::Io(error.to_string()))?,
        };
        let (program, args) = shell_command_argv(&shell, &request.command);
        let mut command = Command::new(program);
        command.args(args);
        command.current_dir(&cwd);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        if request.pipe_stdin {
            command.stdin(Stdio::piped());
        } else {
            command.stdin(Stdio::null());
        }
        for (key, value) in request.env {
            match value {
                Some(value) => {
                    command.env(key, value);
                }
                None => {
                    command.env_remove(key);
                }
            }
        }
        let mut child = command
            .spawn()
            .map_err(|error| ProcessError::Spawn(error.to_string()))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdin = child.stdin.take();
        let foreground_wait_ms = request.foreground_wait_ms;
        let (control, control_rx) = mpsc::unbounded_channel();
        let handle = Arc::new(JobHandle {
            job_id: job_id.clone(),
            command: request.command,
            shell,
            cwd,
            control,
            stdin: Mutex::new(stdin),
            status: Mutex::new(JobStatus::Running),
            ended_at_ms: Mutex::new(None),
            output: Mutex::new(OutputBuffer::new(
                request
                    .max_spool_bytes
                    .unwrap_or(self.inner.default_spool_bytes),
            )),
            notify: Notify::new(),
            started_at_ms: now_ms(),
        });
        self.inner
            .jobs
            .lock()
            .await
            .insert(job_id.clone(), Arc::clone(&handle));
        let mut output_readers = Vec::new();
        if let Some(stdout) = stdout {
            output_readers.push(spawn_output_reader(
                Arc::clone(&handle),
                ProcessOutputStream::Stdout,
                stdout,
            ));
        }
        if let Some(stderr) = stderr {
            output_readers.push(spawn_output_reader(
                Arc::clone(&handle),
                ProcessOutputStream::Stderr,
                stderr,
            ));
        }
        spawn_process_watcher(
            Arc::clone(&self.inner),
            Arc::clone(&handle),
            child,
            control_rx,
            output_readers,
        );
        if let Some(foreground_wait_ms) = foreground_wait_ms {
            let _ = self.wait(&job_id, Some(foreground_wait_ms)).await?;
        }
        Ok(self.snapshot_for_handle(&handle).await)
    }

    pub async fn read(
        &self,
        job_id: &str,
        request: ReadOutputRequest,
    ) -> Result<ProcessOutput, ProcessError> {
        let handle = self.job(job_id).await?;
        self.wait_for_output_if_needed(&handle, &request).await;
        let status = handle.status.lock().await.clone();
        let (chunks, next_cursor, dropped_before_seq, truncated) = handle
            .output
            .lock()
            .await
            .read(request.after_seq.unwrap_or(0), request.max_bytes);
        Ok(ProcessOutput {
            job_id: job_id.into(),
            chunks,
            next_cursor,
            dropped_before_seq,
            truncated,
            status,
        })
    }

    pub async fn wait(
        &self,
        job_id: &str,
        timeout_ms: Option<u64>,
    ) -> Result<WaitOutcome, ProcessError> {
        let handle = self.job(job_id).await?;
        if let Some(timeout_ms) = timeout_ms {
            match timeout(
                Duration::from_millis(timeout_ms),
                wait_until_not_running(&handle),
            )
            .await
            {
                Ok(status) => Ok(WaitOutcome {
                    job_id: job_id.into(),
                    status,
                    timed_out: false,
                }),
                Err(_) => Ok(WaitOutcome {
                    job_id: job_id.into(),
                    status: handle.status.lock().await.clone(),
                    timed_out: true,
                }),
            }
        } else {
            Ok(WaitOutcome {
                job_id: job_id.into(),
                status: wait_until_not_running(&handle).await,
                timed_out: false,
            })
        }
    }

    pub async fn write(&self, job_id: &str, text: &str) -> Result<JobSnapshot, ProcessError> {
        let handle = self.job(job_id).await?;
        let mut stdin = handle.stdin.lock().await;
        let Some(stdin) = stdin.as_mut() else {
            return Err(ProcessError::Invalid(format!(
                "{PROCESS_STDIN_DISABLED_PREFIX}{job_id}{PROCESS_STDIN_DISABLED_SUFFIX}"
            )));
        };
        stdin
            .write_all(text.as_bytes())
            .await
            .map_err(|error| ProcessError::Io(error.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|error| ProcessError::Io(error.to_string()))?;
        Ok(self.snapshot_for_handle(&handle).await)
    }

    pub async fn terminate(&self, job_id: &str) -> Result<JobSnapshot, ProcessError> {
        let handle = self.job(job_id).await?;
        if handle.status.lock().await.is_running()
            && handle.control.send(JobControl::Terminate).is_ok()
        {
            let _ = wait_until_not_running(&handle).await;
        }
        Ok(self.snapshot_for_handle(&handle).await)
    }

    pub async fn list(&self) -> Result<Vec<JobSnapshot>, ProcessError> {
        let handles = self
            .inner
            .jobs
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut snapshots = Vec::with_capacity(handles.len());
        for handle in handles {
            snapshots.push(self.snapshot_for_handle(&handle).await);
        }
        Ok(snapshots)
    }

    pub async fn close(&self) -> Result<(), ProcessError> {
        let job_ids = self
            .inner
            .jobs
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for job_id in job_ids {
            let handle = self.job(&job_id).await?;
            if handle.status.lock().await.is_running() {
                self.terminate(&job_id).await?;
            }
        }
        Ok(())
    }

    async fn wait_for_output_if_needed(
        &self,
        handle: &Arc<JobHandle>,
        request: &ReadOutputRequest,
    ) {
        let Some(wait_ms) = request.wait_ms else {
            return;
        };
        let after_seq = request.after_seq.unwrap_or(0);
        let notified = handle.notify.notified();
        if has_new_output(handle, after_seq).await || !handle.status.lock().await.is_running() {
            return;
        }
        let _ = timeout(Duration::from_millis(wait_ms), notified).await;
    }

    async fn job(&self, job_id: &str) -> Result<Arc<JobHandle>, ProcessError> {
        self.inner
            .jobs
            .lock()
            .await
            .get(job_id)
            .cloned()
            .ok_or_else(|| ProcessError::UnknownJob(job_id.into()))
    }

    async fn snapshot_for_handle(&self, handle: &Arc<JobHandle>) -> JobSnapshot {
        snapshot_for_handle(handle).await
    }
}

impl OutputBuffer {
    fn new(max_bytes: usize) -> Self {
        Self {
            chunks: VecDeque::new(),
            next_seq: 1,
            dropped_before_seq: 0,
            total_bytes: 0,
            max_bytes: max_bytes.max(1024),
        }
    }

    fn push(&mut self, stream: ProcessOutputStream, bytes: &[u8]) {
        let chunk_size = self.max_bytes.max(1);
        for chunk in bytes.chunks(chunk_size) {
            self.push_chunk(stream, chunk);
        }
    }

    fn push_chunk(&mut self, stream: ProcessOutputStream, bytes: &[u8]) {
        let text = text::prefix_to_bytes(&String::from_utf8_lossy(bytes), self.max_bytes);
        let chunk = OutputChunk {
            seq: self.next_seq,
            stream,
            byte_len: text.len(),
            text,
        };
        self.next_seq += 1;
        self.total_bytes += chunk.byte_len;
        self.chunks.push_back(chunk);
        while self.total_bytes > self.max_bytes {
            let Some(removed) = self.chunks.pop_front() else {
                break;
            };
            self.total_bytes = self.total_bytes.saturating_sub(removed.byte_len);
            self.dropped_before_seq = removed.seq;
        }
    }

    fn read(&self, after_seq: u64, max_bytes: Option<usize>) -> (Vec<OutputChunk>, u64, u64, bool) {
        let mut chunks = Vec::new();
        let mut used_bytes = 0usize;
        let mut truncated = after_seq < self.dropped_before_seq;
        let max_bytes = max_bytes.unwrap_or(usize::MAX);
        if max_bytes == 0 {
            return (
                chunks,
                after_seq.max(self.dropped_before_seq),
                self.dropped_before_seq,
                true,
            );
        }
        for chunk in self.chunks.iter().filter(|chunk| chunk.seq > after_seq) {
            let remaining = max_bytes.saturating_sub(used_bytes);
            if remaining == 0 {
                truncated = true;
                break;
            }
            if chunk.byte_len > remaining {
                chunks.push(truncate_chunk(chunk, remaining));
                truncated = true;
                break;
            }
            used_bytes += chunk.byte_len;
            chunks.push(chunk.clone());
        }
        let next_cursor = chunks
            .last()
            .map(|chunk| chunk.seq)
            .unwrap_or(after_seq.max(self.dropped_before_seq));
        (chunks, next_cursor, self.dropped_before_seq, truncated)
    }

    fn tail(&self, max_bytes: usize) -> (Vec<OutputChunk>, u64, u64, bool) {
        let max_bytes = max_bytes.max(1);
        let mut chunks = Vec::new();
        let mut used_bytes = 0usize;
        let mut truncated = self.dropped_before_seq > 0;
        for chunk in self.chunks.iter().rev() {
            let remaining = max_bytes.saturating_sub(used_bytes);
            if remaining == 0 {
                truncated = true;
                break;
            }
            if chunk.byte_len > remaining {
                chunks.push(truncate_chunk_tail(chunk, remaining));
                truncated = true;
                break;
            }
            used_bytes += chunk.byte_len;
            chunks.push(chunk.clone());
        }
        chunks.reverse();
        let next_cursor = self.next_seq.saturating_sub(1);
        if let (Some(first_selected), Some(first_buffered)) = (chunks.first(), self.chunks.front())
            && first_selected.seq > first_buffered.seq
        {
            truncated = true;
        }
        (chunks, next_cursor, self.dropped_before_seq, truncated)
    }
}

fn truncate_chunk(chunk: &OutputChunk, max_bytes: usize) -> OutputChunk {
    let text = text::prefix_to_bytes(&chunk.text, max_bytes);
    OutputChunk {
        seq: chunk.seq,
        stream: chunk.stream,
        byte_len: text.len(),
        text,
    }
}

fn truncate_chunk_tail(chunk: &OutputChunk, max_bytes: usize) -> OutputChunk {
    let text = text::suffix_to_bytes(&chunk.text, max_bytes);
    OutputChunk {
        seq: chunk.seq,
        stream: chunk.stream,
        byte_len: text.len(),
        text,
    }
}

fn spawn_output_reader<R>(
    handle: Arc<JobHandle>,
    stream: ProcessOutputStream,
    mut reader: R,
) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => {
                    handle.output.lock().await.push(stream, &buffer[..read]);
                    handle.notify.notify_waiters();
                }
                Err(error) => {
                    *handle.status.lock().await = JobStatus::Failed {
                        error: error.to_string(),
                    };
                    *handle.ended_at_ms.lock().await = Some(now_ms());
                    handle.notify.notify_waiters();
                    break;
                }
            }
        }
    })
}

fn spawn_process_watcher(
    manager: Arc<ManagerInner>,
    handle: Arc<JobHandle>,
    mut child: Child,
    mut control_rx: mpsc::UnboundedReceiver<JobControl>,
    output_readers: Vec<JoinHandle<()>>,
) {
    tokio::spawn(async move {
        let status = loop {
            tokio::select! {
                wait_result = child.wait() => {
                    break status_from_wait_result(wait_result);
                }
                control = control_rx.recv() => match control {
                    Some(JobControl::Terminate) => break terminate_child(&mut child).await,
                    None => {}
                }
            }
        };
        *handle.stdin.lock().await = None;
        for reader in output_readers {
            let _ = reader.await;
        }
        let status = finish_handle(&handle, status).await;
        publish_process_event(
            &manager,
            HostProcessEvent::JobCompleted {
                completion: completion_for_handle(&handle, status).await,
            },
        );
    });
}

async fn wait_until_not_running(handle: &Arc<JobHandle>) -> JobStatus {
    loop {
        let notified = handle.notify.notified();
        let status = handle.status.lock().await.clone();
        if !status.is_running() {
            return status;
        }
        notified.await;
    }
}

async fn terminate_child(child: &mut Child) -> JobStatus {
    match child.start_kill() {
        Ok(()) => {
            let _ = child.wait().await;
            JobStatus::Terminated
        }
        Err(error) => JobStatus::Failed {
            error: error.to_string(),
        },
    }
}

fn status_from_wait_result(result: std::io::Result<std::process::ExitStatus>) -> JobStatus {
    match result {
        Ok(status) => JobStatus::Exited {
            code: status.code(),
        },
        Err(error) => JobStatus::Failed {
            error: error.to_string(),
        },
    }
}

async fn finish_handle(handle: &Arc<JobHandle>, status: JobStatus) -> JobStatus {
    let mut status_guard = handle.status.lock().await;
    if status_guard.is_running() {
        *status_guard = status;
    }
    let status = status_guard.clone();
    drop(status_guard);
    let mut ended_at_ms = handle.ended_at_ms.lock().await;
    if ended_at_ms.is_none() {
        *ended_at_ms = Some(now_ms());
    }
    handle.notify.notify_waiters();
    status
}

async fn snapshot_for_handle(handle: &Arc<JobHandle>) -> JobSnapshot {
    let status = handle.status.lock().await.clone();
    let ended_at_ms = *handle.ended_at_ms.lock().await;
    let output = handle.output.lock().await;
    JobSnapshot {
        job_id: handle.job_id.clone(),
        command: handle.command.clone(),
        shell: handle.shell.clone(),
        cwd: handle.cwd.clone(),
        status,
        started_at_ms: handle.started_at_ms,
        ended_at_ms,
        next_cursor: output.next_seq.saturating_sub(1),
        dropped_before_seq: output.dropped_before_seq,
    }
}

async fn completion_for_handle(
    handle: &Arc<JobHandle>,
    status: JobStatus,
) -> HostProcessCompletion {
    let snapshot = snapshot_for_handle(handle).await;
    let (chunks, next_cursor, dropped_before_seq, truncated) = handle
        .output
        .lock()
        .await
        .tail(DEFAULT_COMPLETION_TAIL_BYTES);
    HostProcessCompletion {
        snapshot,
        output: ProcessOutput {
            job_id: handle.job_id.clone(),
            chunks,
            next_cursor,
            dropped_before_seq,
            truncated,
            status,
        },
    }
}

fn publish_process_event(manager: &ManagerInner, event: HostProcessEvent) {
    let listeners = manager
        .listeners
        .lock()
        .expect("host process listeners lock poisoned")
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for listener in listeners {
        listener(event.clone());
    }
}

async fn has_new_output(handle: &Arc<JobHandle>, after_seq: u64) -> bool {
    handle
        .output
        .lock()
        .await
        .chunks
        .iter()
        .any(|chunk| chunk.seq > after_seq)
}

fn shell_command_argv(shell: &str, command: &str) -> (String, Vec<String>) {
    let shell_name = shell_executable_name(shell);
    if shell_name == "cmd" || shell_name == "cmd.exe" {
        return (shell.into(), vec!["/C".into(), command.into()]);
    }
    if shell_name.starts_with("powershell") || shell_name == "pwsh" || shell_name == "pwsh.exe" {
        return (
            shell.into(),
            vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-Command".into(),
                command.into(),
            ],
        );
    }
    (shell.into(), vec!["-lc".into(), command.into()])
}

pub(crate) fn shell_executable_name(shell: &str) -> String {
    PathBuf::from(shell)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| shell.to_ascii_lowercase())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessError {
    Invalid(String),
    UnknownJob(JobId),
    Spawn(String),
    Io(String),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "invalid process request: {message}"),
            Self::UnknownJob(job_id) => write!(formatter, "unknown process job: {job_id}"),
            Self::Spawn(message) => write!(formatter, "failed to spawn process: {message}"),
            Self::Io(message) => write!(formatter, "process io failed: {message}"),
        }
    }
}

impl std::error::Error for ProcessError {}

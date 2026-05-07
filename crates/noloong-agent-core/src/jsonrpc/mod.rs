mod adapters;
mod process;
mod wire;

use crate::{
    AgentCoreError, ExtensionCapability, ExtensionCapabilitySelector, ExtensionManifest,
    ModelStreamEvent, Result,
};
use crate::{CancellationToken, ModelStreamSink};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, ChildStdin, Command},
    sync::{Mutex, mpsc, oneshot},
    time::timeout,
};

pub use adapters::{
    StdioCompactionSummarizer, StdioContextCompactor, StdioContextProvider, StdioHttpAuthProvider,
    StdioModelProvider, StdioPhaseHook, StdioPhaseNode, StdioToolCallHook, StdioToolProvider,
};
use wire::{CapabilitiesResult, InitializeResult, JsonRpcRequest};

const MODEL_STREAM_EVENT_BUFFER_CAPACITY: usize = 128;

#[derive(Clone, Debug)]
pub struct StdioExtensionConfig {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub clear_env: bool,
    pub allowed_capabilities: Option<BTreeSet<ExtensionCapabilitySelector>>,
    pub request_timeout: Duration,
    pub stream_timeout: Duration,
}

impl StdioExtensionConfig {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            clear_env: false,
            allowed_capabilities: None,
            request_timeout: Duration::from_secs(5),
            stream_timeout: Duration::from_secs(30),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    pub fn envs(
        mut self,
        env: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        self.env.extend(
            env.into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        );
        self
    }

    pub fn clear_env(mut self, clear_env: bool) -> Self {
        self.clear_env = clear_env;
        self
    }

    pub fn allowed_capabilities(
        mut self,
        allowed_capabilities: BTreeSet<ExtensionCapabilitySelector>,
    ) -> Self {
        self.allowed_capabilities = Some(allowed_capabilities);
        self
    }

    pub fn request_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }

    pub fn stream_timeout(mut self, stream_timeout: Duration) -> Self {
        self.stream_timeout = stream_timeout;
        self
    }
}

pub struct StdioExtension {
    manifest: ExtensionManifest,
    writer: Arc<Mutex<ChildStdin>>,
    pending: PendingRequests,
    model_stream_sinks: ModelStreamRegistrations,
    request_counter: AtomicU64,
    request_timeout: Duration,
    stream_timeout: Duration,
    _child: Mutex<Child>,
}

type PendingRequests = Arc<Mutex<BTreeMap<u64, oneshot::Sender<Result<Value>>>>>;
type ModelStreamRegistrations = Arc<Mutex<BTreeMap<String, ModelStreamRegistration>>>;

#[derive(Clone)]
struct ModelStreamRegistration {
    sink: ModelStreamSink,
    events: mpsc::Sender<Result<ModelStreamEvent>>,
}

impl StdioExtension {
    pub async fn connect(config: StdioExtensionConfig) -> Result<Self> {
        let mut command = Command::new(&config.command);
        if let Some(cwd) = &config.cwd {
            command.current_dir(cwd);
        }
        if config.clear_env {
            command.env_clear();
        }
        command
            .args(&config.args)
            .envs(&config.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AgentCoreError::JsonRpc("extension stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AgentCoreError::JsonRpc("extension stdout unavailable".into()))?;

        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let model_stream_sinks = Arc::new(Mutex::new(BTreeMap::new()));
        tokio::spawn(process::read_stdout(
            stdout,
            pending.clone(),
            model_stream_sinks.clone(),
        ));

        let extension = Self {
            manifest: ExtensionManifest {
                name: config.command.clone(),
                version: "unknown".into(),
            },
            writer: Arc::new(Mutex::new(stdin)),
            pending,
            model_stream_sinks,
            request_counter: AtomicU64::new(0),
            request_timeout: config.request_timeout,
            stream_timeout: config.stream_timeout,
            _child: Mutex::new(child),
        };

        let manifest = extension
            .request::<InitializeResult>("initialize", json!({ "protocolVersion": 1 }), None)
            .await?
            .manifest;

        Ok(Self {
            manifest,
            ..extension
        })
    }

    pub fn manifest(&self) -> &ExtensionManifest {
        &self.manifest
    }

    pub async fn capabilities(&self) -> Result<Vec<ExtensionCapability>> {
        Ok(self
            .request::<CapabilitiesResult>("capabilities/list", json!({}), None)
            .await?
            .capabilities)
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.request::<Value>("shutdown", json!({}), None).await?;
        Ok(())
    }

    async fn request<T>(
        &self,
        method: &str,
        params: Value,
        cancellation: Option<CancellationToken>,
    ) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let id = self.next_request_id();
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let payload = serde_json::to_vec(&request)?;
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);

        let write_result = {
            let mut writer = self.writer.lock().await;
            async {
                writer.write_all(&payload).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await
            }
            .await
        };
        if let Err(error) = write_result {
            self.pending.lock().await.remove(&id);
            return Err(error.into());
        }

        let response = if let Some(cancellation) = cancellation {
            tokio::select! {
                response = timeout(self.request_timeout, receiver) => {
                    match response {
                        Ok(Ok(response)) => response,
                        Ok(Err(_)) => {
                            self.pending.lock().await.remove(&id);
                            return Err(AgentCoreError::JsonRpc(format!("response channel closed: {method}")));
                        }
                        Err(_) => {
                            self.pending.lock().await.remove(&id);
                            return Err(AgentCoreError::JsonRpc(format!("request timed out: {method}")));
                        }
                    }
                }
                _ = cancellation.cancelled() => {
                    self.pending.lock().await.remove(&id);
                    return Err(AgentCoreError::Aborted);
                }
            }
        } else {
            match timeout(self.request_timeout, receiver).await {
                Ok(Ok(response)) => response,
                Ok(Err(_)) => {
                    self.pending.lock().await.remove(&id);
                    return Err(AgentCoreError::JsonRpc(format!(
                        "response channel closed: {method}"
                    )));
                }
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    return Err(AgentCoreError::JsonRpc(format!(
                        "request timed out: {method}"
                    )));
                }
            }
        }?;
        Ok(serde_json::from_value(response)?)
    }

    async fn register_model_stream(
        &self,
        stream_id: String,
        sink: ModelStreamSink,
    ) -> mpsc::Receiver<Result<ModelStreamEvent>> {
        let (sender, receiver) = mpsc::channel(MODEL_STREAM_EVENT_BUFFER_CAPACITY);
        self.model_stream_sinks.lock().await.insert(
            stream_id,
            ModelStreamRegistration {
                sink,
                events: sender,
            },
        );
        receiver
    }

    async fn unregister_model_stream(&self, stream_id: &str) {
        self.model_stream_sinks.lock().await.remove(stream_id);
    }

    fn next_request_id(&self) -> u64 {
        self.request_counter.fetch_add(1, Ordering::SeqCst) + 1
    }
}

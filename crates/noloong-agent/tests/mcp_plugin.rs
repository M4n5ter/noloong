#![cfg(feature = "mcp")]

use noloong_agent::{
    AgentManifest, AgentPluginDeclaration, AgentSession, McpHeaderSource, McpPluginComponent,
    McpPluginTransport, McpStdioTransport, McpStreamableHttpTransport, PluginComponent,
    PluginEnvSource, PluginLoadFailurePolicy,
};
use noloong_agent_core::{
    AgentState, BoxFuture, CancellationToken, ContentBlock, ModelProvider, ModelRequest,
    ModelStreamEvent, ModelStreamSink, Result, StopReason, ToolRequest,
};
use rmcp::{
    ErrorData as McpError, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, Content, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, Tool,
    },
    service::{RequestContext, RoleServer},
    transport::{
        StreamableHttpServerConfig,
        streamable_http_server::{
            session::local::LocalSessionManager, tower::StreamableHttpService,
        },
    },
};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    fs,
    future::{Future, ready},
    path::{Path, PathBuf},
    sync::Arc,
};

#[tokio::test]
async fn stdio_mcp_component_registers_and_calls_tool() -> Result<()> {
    let server_path = write_stdio_mcp_server(&temp_dir("stdio"));
    let runtime = runtime_with_plugin(mcp_stdio_plugin(server_path, Some("stdio"))).await?;

    let tool = runtime.tool("stdio.echo")?;
    let spec = tool.spec();
    assert_eq!(spec.name, "stdio.echo");
    assert_eq!(spec.permissions[0].capability, "mcp.tool");
    assert_eq!(
        spec.permissions[0].metadata["pluginId"],
        Value::String("mcp-test".into())
    );

    let output = tool
        .execute_tool(
            tool_request("stdio.echo", json!({"text": "hello"})),
            CancellationToken::new(),
        )
        .await?;

    assert!(!output.is_error);
    assert_eq!(text_output(&output.content), "stdio:hello");
    Ok(())
}

#[tokio::test]
async fn mcp_component_honors_enabled_and_disabled_tools() -> Result<()> {
    let server_path = write_stdio_mcp_server(&temp_dir("filters"));
    let mut plugin = mcp_stdio_plugin(server_path, Some("picked"));
    let component = mcp_component_mut(&mut plugin);
    component.enabled_tools = vec!["echo".into(), "blocked".into()];
    component.disabled_tools = vec!["blocked".into()];

    let runtime = runtime_with_plugin(plugin).await?;

    assert!(runtime.tool("picked.echo").is_ok());
    assert!(runtime.tool("picked.blocked").is_err());
    Ok(())
}

#[tokio::test]
async fn mcp_tool_name_collisions_fail_runtime_build() {
    let server_path = write_stdio_mcp_server(&temp_dir("collision"));
    let mut plugin = mcp_stdio_plugin(server_path, Some("dup"));
    let duplicate_component = mcp_component_mut(&mut plugin).clone();
    plugin
        .components
        .push(PluginComponent::Mcp(duplicate_component));

    let manifest = AgentManifest::default().with_plugin(plugin).unwrap();
    let session = AgentSession::builder().with_manifest(manifest).build();

    let error = match session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .with_manifest_plugins()
        .await
    {
        Ok(_) => panic!("duplicate MCP tool name should fail runtime build"),
        Err(error) => error,
    };

    assert!(
        error
            .to_string()
            .contains("conflicts with an existing runtime tool")
    );
}

#[tokio::test]
async fn streamable_http_mcp_component_registers_and_calls_tool() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let service: StreamableHttpService<HttpMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(HttpMcpServer::new()),
            Default::default(),
            StreamableHttpServerConfig::default().with_sse_keep_alive(None),
        );
    let router = axum::Router::new().nest_service("/mcp", service);
    let server_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    let runtime =
        runtime_with_plugin(mcp_streamable_http_plugin(format!("http://{addr}/mcp"))).await?;
    let tool = runtime.tool("http.echo")?;
    let output = tool
        .execute_tool(
            tool_request("http.echo", json!({"text": "hello"})),
            CancellationToken::new(),
        )
        .await?;

    server_handle.abort();
    let _ = server_handle.await;

    assert!(!output.is_error);
    assert_eq!(text_output(&output.content), "http:hello");
    Ok(())
}

#[tokio::test]
async fn streamable_http_header_secret_is_redacted_in_summary() {
    let plugin = mcp_streamable_http_plugin_with_header("http://127.0.0.1:9/mcp".into());
    let summary = plugin.summary();

    assert!(summary.contains("Authorization<=host_env:MCP_TOKEN"));
    assert!(!summary.contains("Bearer "));
    assert!(!summary.contains("secret"));
}

async fn runtime_with_plugin(
    plugin: AgentPluginDeclaration,
) -> Result<noloong_agent_core::AgentRuntime> {
    let manifest = AgentManifest::default().with_plugin(plugin).unwrap();
    let session = AgentSession::builder().with_manifest(manifest).build();
    session
        .runtime_builder()
        .with_model_provider(Arc::new(DummyModelProvider))
        .with_manifest_plugins()
        .await?
        .build()
}

fn mcp_stdio_plugin(
    server_path: PathBuf,
    tool_name_prefix: Option<&str>,
) -> AgentPluginDeclaration {
    AgentPluginDeclaration {
        plugin_id: "mcp-test".into(),
        display_name: "MCP Test".into(),
        description: None,
        components: vec![PluginComponent::Mcp(McpPluginComponent {
            server_id: "stdio-server".into(),
            transport: McpPluginTransport::Stdio(McpStdioTransport {
                command: "node".into(),
                args: vec![server_path.to_string_lossy().into_owned()],
                cwd: None,
                env: BTreeMap::from([(
                    "PATH".into(),
                    PluginEnvSource::HostEnv {
                        name: "PATH".into(),
                    },
                )]),
                request_timeout_secs: Some(5),
            }),
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            tool_name_prefix: tool_name_prefix.map(str::to_string),
            request_timeout_secs: Some(5),
        })],
        enabled: true,
        on_load_failure: PluginLoadFailurePolicy::FailRun,
    }
}

fn mcp_streamable_http_plugin(url: String) -> AgentPluginDeclaration {
    mcp_streamable_http_plugin_inner(url, BTreeMap::new())
}

fn mcp_streamable_http_plugin_with_header(url: String) -> AgentPluginDeclaration {
    mcp_streamable_http_plugin_inner(
        url,
        BTreeMap::from([(
            "Authorization".into(),
            McpHeaderSource::HostEnv {
                name: "MCP_TOKEN".into(),
                prefix: Some("Bearer ".into()),
            },
        )]),
    )
}

fn mcp_streamable_http_plugin_inner(
    url: String,
    headers: BTreeMap<String, McpHeaderSource>,
) -> AgentPluginDeclaration {
    AgentPluginDeclaration {
        plugin_id: "mcp-http-test".into(),
        display_name: "MCP HTTP Test".into(),
        description: None,
        components: vec![PluginComponent::Mcp(McpPluginComponent {
            server_id: "http-server".into(),
            transport: McpPluginTransport::StreamableHttp(McpStreamableHttpTransport {
                url,
                headers,
                connect_timeout_secs: Some(5),
                request_timeout_secs: Some(5),
            }),
            enabled_tools: Vec::new(),
            disabled_tools: Vec::new(),
            tool_name_prefix: Some("http".into()),
            request_timeout_secs: Some(5),
        })],
        enabled: true,
        on_load_failure: PluginLoadFailurePolicy::FailRun,
    }
}

fn mcp_component_mut(plugin: &mut AgentPluginDeclaration) -> &mut McpPluginComponent {
    plugin
        .components
        .iter_mut()
        .find_map(|component| match component {
            PluginComponent::Mcp(component) => Some(component),
            _ => None,
        })
        .expect("test plugin has mcp component")
}

fn tool_request(tool_name: &str, arguments: Value) -> ToolRequest {
    ToolRequest {
        run_id: "run-test".into(),
        turn_id: 1,
        tool_call_id: "call-test".into(),
        tool_name: tool_name.into(),
        arguments,
        state: AgentState::default(),
    }
}

fn text_output(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_stdio_mcp_server(root: &Path) -> PathBuf {
    let path = root.join("server.mjs");
    fs::write(
        &path,
        r#"
import readline from 'node:readline';

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });

function write(message) {
  process.stdout.write(JSON.stringify(message) + '\n');
}

function result(id, value) {
  write({ jsonrpc: '2.0', id, result: value });
}

rl.on('line', (line) => {
  if (!line.trim()) return;
  const request = JSON.parse(line);
  if (request.id === undefined) return;
  if (request.method === 'initialize') {
    result(request.id, {
      protocolVersion: '2025-06-18',
      capabilities: { tools: {} },
      serverInfo: { name: 'fake-mcp', version: '1.0.0' }
    });
    return;
  }
  if (request.method === 'tools/list') {
    result(request.id, {
      tools: [
        {
          name: 'echo',
          description: 'Echo input text',
          inputSchema: {
            type: 'object',
            properties: { text: { type: 'string' } },
            required: ['text']
          }
        },
        {
          name: 'blocked',
          description: 'Blocked test tool',
          inputSchema: { type: 'object', properties: {} }
        }
      ]
    });
    return;
  }
  if (request.method === 'tools/call') {
    const text = request.params?.arguments?.text ?? '';
    result(request.id, {
      content: [{ type: 'text', text: `stdio:${text}` }],
      isError: false
    });
    return;
  }
  write({
    jsonrpc: '2.0',
    id: request.id,
    error: { code: -32601, message: `unknown method ${request.method}` }
  });
});
"#,
    )
    .unwrap();
    path
}

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("noloong-mcp-{name}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}

#[derive(Clone)]
struct HttpMcpServer;

impl HttpMcpServer {
    fn new() -> Self {
        Self
    }
}

impl ServerHandler for HttpMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListToolsResult, McpError>> + Send + '_ {
        ready(Ok(ListToolsResult::with_all_items(vec![self.echo_tool()])))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        (name == "echo").then(|| self.echo_tool())
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<CallToolResult, McpError>> + Send + '_ {
        let text = request
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        ready(Ok(CallToolResult::success(vec![Content::text(format!(
            "http:{text}"
        ))])))
    }
}

impl HttpMcpServer {
    fn echo_tool(&self) -> Tool {
        Tool::new(
            "echo",
            "Echo input text",
            json_object(json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"]
            })),
        )
    }
}

fn json_object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

struct DummyModelProvider;

impl ModelProvider for DummyModelProvider {
    fn id(&self) -> &str {
        "dummy-model"
    }

    fn stream_model<'a>(
        &'a self,
        _request: ModelRequest,
        sink: ModelStreamSink,
        _cancellation: CancellationToken,
    ) -> BoxFuture<'a, Vec<ModelStreamEvent>> {
        Box::pin(async move {
            let event = ModelStreamEvent::Finished {
                stop_reason: StopReason::Stop,
            };
            sink(event.clone()).await?;
            Ok(vec![event])
        })
    }
}

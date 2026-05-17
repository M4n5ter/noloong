use crate::{
    McpPluginComponent, McpPluginTransport, McpStdioTransport, McpStreamableHttpTransport,
    PluginLoadError, tools::json_tool_error,
};
use noloong_agent_core::{
    BoxFuture, CancellationToken, ContentBlock, ToolOutput, ToolPermissionRequirement,
    ToolProvider, ToolRequest, ToolSpec,
};
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, RawContent, Tool},
    service::{Peer, RunningService},
    transport::{
        ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use tokio::{process::Command, sync::Mutex};

const DEFAULT_MCP_REQUEST_TIMEOUT_SECS: u64 = 30;
const MCP_PERMISSION_CAPABILITY: &str = "mcp.tool";

pub async fn load_mcp_tools(
    plugin_id: &str,
    plugin_display_name: &str,
    component: &McpPluginComponent,
    env_source: impl Fn(&str) -> Option<String>,
) -> Result<Vec<Arc<dyn ToolProvider>>, PluginLoadError> {
    let server = connect_mcp_server(plugin_id, plugin_display_name, component, env_source).await?;
    let tools = server
        .list_tools()
        .await
        .map_err(|message| PluginLoadError::Startup {
            plugin_id: plugin_id.into(),
            message,
        })?;
    let selected = select_tools(plugin_id, component, tools)?;
    let mut providers = Vec::with_capacity(selected.len());
    for tool in selected {
        providers.push(Arc::new(McpToolProvider {
            exposed_name: exposed_tool_name(plugin_id, component, &tool.name),
            original_name: tool.name.to_string(),
            description: tool
                .description
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("MCP tool `{}`", tool.name)),
            input_schema: Value::Object(tool.input_schema.as_ref().clone()),
            server: Arc::clone(&server),
        }) as Arc<dyn ToolProvider>);
    }
    Ok(providers)
}

async fn connect_mcp_server(
    plugin_id: &str,
    plugin_display_name: &str,
    component: &McpPluginComponent,
    env_source: impl Fn(&str) -> Option<String>,
) -> Result<Arc<McpServerRuntime>, PluginLoadError> {
    let timeout = Duration::from_secs(request_timeout_secs(component));
    match &component.transport {
        McpPluginTransport::Stdio(transport) => {
            let running = connect_stdio(plugin_id, transport, timeout, &env_source).await?;
            Ok(Arc::new(McpServerRuntime {
                plugin_id: plugin_id.into(),
                plugin_display_name: plugin_display_name.into(),
                server_id: component.server_id.clone(),
                request_timeout: timeout,
                peer: running.peer().clone(),
                _running: Mutex::new(running),
            }))
        }
        McpPluginTransport::StreamableHttp(transport) => {
            let running =
                connect_streamable_http(plugin_id, transport, timeout, &env_source).await?;
            Ok(Arc::new(McpServerRuntime {
                plugin_id: plugin_id.into(),
                plugin_display_name: plugin_display_name.into(),
                server_id: component.server_id.clone(),
                request_timeout: timeout,
                peer: running.peer().clone(),
                _running: Mutex::new(running),
            }))
        }
    }
}

fn request_timeout_secs(component: &McpPluginComponent) -> u64 {
    component
        .request_timeout_secs
        .or(match &component.transport {
            McpPluginTransport::Stdio(transport) => transport.request_timeout_secs,
            McpPluginTransport::StreamableHttp(transport) => transport.request_timeout_secs,
        })
        .unwrap_or(DEFAULT_MCP_REQUEST_TIMEOUT_SECS)
}

async fn connect_stdio(
    plugin_id: &str,
    transport: &McpStdioTransport,
    timeout: Duration,
    env_source: &impl Fn(&str) -> Option<String>,
) -> Result<RunningService<RoleClient, ()>, PluginLoadError> {
    let mut command = Command::new(&transport.command);
    command.args(&transport.args);
    command.kill_on_drop(true);
    if let Some(cwd) = &transport.cwd {
        command.current_dir(cwd);
    }
    command.env_clear();
    for (target_name, source) in &transport.env {
        let value = source.resolve(plugin_id, target_name, env_source)?;
        command.env(target_name, value);
    }
    let child = TokioChildProcess::new(command.configure(|_| {})).map_err(|error| {
        PluginLoadError::Startup {
            plugin_id: plugin_id.into(),
            message: format!("stdio MCP server failed to start: {error}"),
        }
    })?;
    timeout_initialize(plugin_id, timeout, ().serve(child)).await
}

async fn connect_streamable_http(
    plugin_id: &str,
    transport: &McpStreamableHttpTransport,
    timeout: Duration,
    env_source: &impl Fn(&str) -> Option<String>,
) -> Result<RunningService<RoleClient, ()>, PluginLoadError> {
    let mut headers = BTreeMap::new();
    for (name, source) in &transport.headers {
        headers.insert(name.clone(), source.resolve(plugin_id, name, env_source)?);
    }
    let mut header_map = std::collections::HashMap::new();
    for (name, value) in headers {
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            PluginLoadError::Startup {
                plugin_id: plugin_id.into(),
                message: format!("invalid MCP header name `{name}`: {error}"),
            }
        })?;
        let value = reqwest::header::HeaderValue::from_str(&value).map_err(|error| {
            PluginLoadError::Startup {
                plugin_id: plugin_id.into(),
                message: format!("invalid MCP header value for `{name}`: {error}"),
            }
        })?;
        header_map.insert(name, value);
    }
    let config = StreamableHttpClientTransportConfig::with_uri(transport.url.clone())
        .custom_headers(header_map);
    let client = if let Some(connect_timeout_secs) = transport.connect_timeout_secs {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(connect_timeout_secs))
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|error| PluginLoadError::Startup {
                plugin_id: plugin_id.into(),
                message: format!("failed to build MCP HTTP client: {error}"),
            })?
    } else {
        reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|error| PluginLoadError::Startup {
                plugin_id: plugin_id.into(),
                message: format!("failed to build MCP HTTP client: {error}"),
            })?
    };
    let transport = StreamableHttpClientTransport::with_client(client, config);
    timeout_initialize(plugin_id, timeout, ().serve(transport)).await
}

async fn timeout_initialize<F>(
    plugin_id: &str,
    timeout: Duration,
    future: F,
) -> Result<RunningService<RoleClient, ()>, PluginLoadError>
where
    F: std::future::Future<
            Output = Result<RunningService<RoleClient, ()>, rmcp::service::ClientInitializeError>,
        >,
{
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| PluginLoadError::Startup {
            plugin_id: plugin_id.into(),
            message: format!(
                "MCP server initialize timed out after {}s",
                timeout.as_secs()
            ),
        })?
        .map_err(|error| PluginLoadError::Startup {
            plugin_id: plugin_id.into(),
            message: format!("MCP server initialize failed: {error}"),
        })
}

fn select_tools(
    plugin_id: &str,
    component: &McpPluginComponent,
    tools: Vec<Tool>,
) -> Result<Vec<Tool>, PluginLoadError> {
    let enabled = component
        .enabled_tools
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let disabled = component
        .disabled_tools
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut selected = Vec::new();
    for tool in tools {
        let original_name = tool.name.to_string();
        if disabled.contains(&original_name)
            || (!enabled.is_empty() && !enabled.contains(&original_name))
        {
            continue;
        }
        let exposed_name = exposed_tool_name(plugin_id, component, &tool.name);
        if !seen.insert(exposed_name.clone()) {
            return Err(PluginLoadError::Startup {
                plugin_id: plugin_id.into(),
                message: format!("duplicate MCP exposed tool name `{exposed_name}`"),
            });
        }
        selected.push(tool);
    }
    Ok(selected)
}

fn exposed_tool_name(
    plugin_id: &str,
    component: &McpPluginComponent,
    original_name: &str,
) -> String {
    let prefix = component
        .tool_name_prefix
        .clone()
        .unwrap_or_else(|| format!("mcp.{plugin_id}.{}", component.server_id));
    format!(
        "{}.{}",
        normalize_tool_segment(&prefix),
        normalize_tool_segment(original_name)
    )
}

fn normalize_tool_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

struct McpServerRuntime {
    plugin_id: String,
    plugin_display_name: String,
    server_id: String,
    request_timeout: Duration,
    peer: Peer<RoleClient>,
    _running: Mutex<RunningService<RoleClient, ()>>,
}

impl McpServerRuntime {
    async fn list_tools(&self) -> Result<Vec<Tool>, String> {
        tokio::time::timeout(self.request_timeout, self.peer.list_all_tools())
            .await
            .map_err(|_| {
                format!(
                    "MCP server `{}` list_tools timed out after {}s",
                    self.server_id,
                    self.request_timeout.as_secs()
                )
            })?
            .map_err(|error| format!("MCP server `{}` list_tools failed: {error}", self.server_id))
    }

    async fn call_tool(
        &self,
        name: String,
        arguments: Map<String, Value>,
    ) -> Result<CallToolResult, String> {
        let mut params = CallToolRequestParams::new(name);
        if !arguments.is_empty() {
            params = params.with_arguments(arguments);
        }
        tokio::time::timeout(self.request_timeout, self.peer.call_tool(params))
            .await
            .map_err(|_| {
                format!(
                    "MCP server `{}` tool call timed out after {}s",
                    self.server_id,
                    self.request_timeout.as_secs()
                )
            })?
            .map_err(|error| format!("MCP server `{}` tool call failed: {error}", self.server_id))
    }
}

struct McpToolProvider {
    exposed_name: String,
    original_name: String,
    description: String,
    input_schema: Value,
    server: Arc<McpServerRuntime>,
}

impl ToolProvider for McpToolProvider {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.exposed_name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            execution_mode: None,
            permissions: vec![ToolPermissionRequirement {
                capability: MCP_PERMISSION_CAPABILITY.into(),
                description: Some(format!(
                    "Call MCP tool `{}` from plugin `{}` server `{}`.",
                    self.original_name, self.server.plugin_display_name, self.server.server_id
                )),
                metadata: json!({
                    "builtIn": false,
                    "capability": MCP_PERMISSION_CAPABILITY,
                    "pluginId": self.server.plugin_id,
                    "pluginDisplayName": self.server.plugin_display_name,
                    "serverId": self.server.server_id,
                    "tool": self.original_name,
                }),
            }],
        }
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let arguments = match request.arguments {
                Value::Object(arguments) => arguments,
                Value::Null => Map::new(),
                value => {
                    return Ok(json_tool_error(
                        "mcp_invalid_arguments",
                        "MCP tool arguments must be a JSON object",
                        json!({
                            "pluginId": self.server.plugin_id,
                            "serverId": self.server.server_id,
                            "tool": self.original_name,
                            "received": value,
                        }),
                    ));
                }
            };
            match self
                .server
                .call_tool(self.original_name.clone(), arguments)
                .await
            {
                Ok(result) => Ok(call_tool_result_output(result)),
                Err(error) => Ok(json_tool_error(
                    "mcp_call_failed",
                    error,
                    json!({
                        "pluginId": self.server.plugin_id,
                        "serverId": self.server.server_id,
                        "tool": self.original_name,
                    }),
                )),
            }
        })
    }
}

fn call_tool_result_output(result: CallToolResult) -> ToolOutput {
    let CallToolResult {
        content: raw_content,
        structured_content,
        is_error,
        meta,
        ..
    } = result;
    let content_count = raw_content.len();
    let mut content = raw_content
        .into_iter()
        .map(mcp_content_block)
        .collect::<Vec<_>>();
    let has_structured_content = structured_content.is_some();
    if let Some(value) = structured_content {
        content.push(ContentBlock::Json { value });
    }
    let is_error = is_error.unwrap_or(false);
    let details = json!({
        "contentCount": content_count,
        "hasStructuredContent": has_structured_content,
        "isError": is_error,
        "meta": meta,
    });
    if content.is_empty() {
        content.push(ContentBlock::Json {
            value: details.clone(),
        });
    }
    ToolOutput {
        content,
        details,
        is_error,
        updates: Vec::new(),
    }
}

fn mcp_content_block(content: rmcp::model::Content) -> ContentBlock {
    match content.raw {
        RawContent::Text(text) => ContentBlock::Text { text: text.text },
        RawContent::Image(image) => ContentBlock::Json {
            value: json!({
                "type": "image",
                "mimeType": image.mime_type,
                "data": image.data,
            }),
        },
        RawContent::Audio(audio) => ContentBlock::Json {
            value: json!({
                "type": "audio",
                "mimeType": audio.mime_type,
                "data": audio.data,
            }),
        },
        RawContent::Resource(resource) => ContentBlock::Json {
            value: serde_json::to_value(resource).unwrap_or_else(|_| json!({})),
        },
        RawContent::ResourceLink(resource) => ContentBlock::Json {
            value: serde_json::to_value(resource).unwrap_or_else(|_| json!({})),
        },
    }
}

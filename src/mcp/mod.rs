//! Helpers for building MCP-compatible tooling around the SDK.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Future;
use serde_json::{json, Map, Value};

use crate::error::SdkError;

/// Metadata describing an MCP tool exposed by an SDK server.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<Value>,
}

impl McpToolInfo {
    pub fn new(
        name: impl Into<String>,
        description: Option<String>,
        input_schema: Option<Value>,
    ) -> Self {
        Self {
            name: name.into(),
            description,
            input_schema,
        }
    }
}

/// Content payloads returned from MCP tool executions.
#[derive(Debug, Clone, PartialEq)]
pub enum McpToolContent {
    Text { text: String },
    Image { data: String, mime_type: String },
    Json { value: Value },
}

impl McpToolContent {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Image {
            data: data.into(),
            mime_type: mime_type.into(),
        }
    }

    pub fn json(value: Value) -> Self {
        Self::Json { value }
    }
}

/// Result of invoking an MCP tool.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolCallResult {
    pub content: Vec<McpToolContent>,
    pub is_error: bool,
}

impl McpToolCallResult {
    pub fn new(content: Vec<McpToolContent>) -> Self {
        Self {
            content,
            is_error: false,
        }
    }

    pub fn with_error(mut self, is_error: bool) -> Self {
        self.is_error = is_error;
        self
    }
}

/// Future type returned by SDK MCP tool handlers.
pub type ToolFuture = Pin<Box<dyn Future<Output = Result<McpToolCallResult, SdkError>> + Send>>;

/// Definition of an SDK MCP tool that can be registered with a server.
#[derive(Clone)]
pub struct SdkMcpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub handler: Arc<dyn Fn(Map<String, Value>) -> ToolFuture + Send + Sync>,
}

impl SdkMcpTool {
    pub fn new<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
        handler: F,
    ) -> Self
    where
        F: Fn(Map<String, Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<McpToolCallResult, SdkError>> + Send + 'static,
    {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            handler: Arc::new(move |args| Box::pin(handler(args))),
        }
    }
}

/// Convenience factory emulating the Python `@tool` decorator.
pub fn tool<F, Fut>(
    name: impl Into<String>,
    description: impl Into<String>,
    input_schema: Value,
    handler: F,
) -> SdkMcpTool
where
    F: Fn(Map<String, Value>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<McpToolCallResult, SdkError>> + Send + 'static,
{
    SdkMcpTool::new(name, description, input_schema, handler)
}

/// Trait implemented by MCP servers hosted inside the SDK process.
#[async_trait]
pub trait SdkMcpServer: Send + Sync {
    /// Human readable server name.
    fn name(&self) -> &str;

    /// Optional semantic version string for the server.
    fn version(&self) -> Option<&str> {
        None
    }

    /// List the tools made available by this server.
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, SdkError>;

    /// Invoke a tool exposed by this server.
    async fn call_tool(
        &self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> Result<McpToolCallResult, SdkError>;
}

/// In-process MCP server implementation.
struct InProcessMcpServer {
    name: String,
    version: String,
    tools: Vec<SdkMcpTool>,
}

#[async_trait]
impl SdkMcpServer for InProcessMcpServer {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> Option<&str> {
        Some(&self.version)
    }

    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, SdkError> {
        Ok(self
            .tools
            .iter()
            .map(|tool| {
                McpToolInfo::new(
                    tool.name.clone(),
                    Some(tool.description.clone()),
                    Some(tool.input_schema.clone()),
                )
            })
            .collect())
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: Map<String, Value>,
    ) -> Result<McpToolCallResult, SdkError> {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.name == name)
            .ok_or_else(|| SdkError::Message(format!("Tool '{name}' not found")))?;
        (tool.handler)(arguments).await
    }
}

/// Create an in-process MCP server that can be registered with [`ClaudeAgentOptions`].
pub fn create_sdk_mcp_server(
    name: impl Into<String>,
    version: impl Into<String>,
    tools: Vec<SdkMcpTool>,
) -> Arc<dyn SdkMcpServer> {
    Arc::new(InProcessMcpServer {
        name: name.into(),
        version: version.into(),
        tools,
    })
}

/// Helper to build a simple JSON schema map from parameter names to types.
pub fn simple_input_schema(params: &[(&str, &str)]) -> Value {
    let mut properties = Map::new();
    for (name, ty) in params {
        let schema = match *ty {
            "string" => json!({"type": "string"}),
            "number" => json!({"type": "number"}),
            "integer" => json!({"type": "integer"}),
            "boolean" => json!({"type": "boolean"}),
            other => json!({"type": other}),
        };
        properties.insert((*name).to_string(), schema);
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": params.iter().map(|(name, _)| name.to_string()).collect::<Vec<_>>(),
    })
}

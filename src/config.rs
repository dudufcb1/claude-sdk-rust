//! Configuration types mirroring the Python `ClaudeAgentOptions` structure.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hooks::{HookEvent, HookMatcher};
use crate::mcp::SdkMcpServer;
use crate::permission::{CanUseToolHandle, PermissionMode, PermissionUpdate};

/// Source of configuration settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingSource {
    User,
    Project,
    Local,
}

/// Preset system prompt configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemPromptPreset {
    #[serde(rename = "type")]
    pub kind: SystemPromptPresetType,
    pub preset: SystemPromptPresetName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub append: Option<String>,
}

/// Type discriminator for system prompt presets.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SystemPromptPresetType {
    #[serde(rename = "preset")]
    Preset,
}

/// Supported preset names.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SystemPromptPresetName {
    #[serde(rename = "claude_code")]
    ClaudeCode,
}

/// Representation of the system prompt option supplied to the SDK.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SystemPrompt {
    Text(String),
    Preset(SystemPromptPreset),
}

/// Agent definition configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentDefinition {
    pub description: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Destination behaviour for SDK MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpStdioServerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpSseServerConfig {
    #[serde(rename = "type")]
    pub kind: McpServerKind,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpHttpServerConfig {
    #[serde(rename = "type")]
    pub kind: McpServerKind,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpSdkServerConfig {
    #[serde(rename = "type")]
    pub kind: McpServerKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum McpServerKind {
    #[serde(rename = "stdio")]
    Stdio,
    #[serde(rename = "sse")]
    Sse,
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "sdk")]
    Sdk,
}

/// Union of supported MCP server configurations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum McpServerConfig {
    #[serde(rename = "stdio")]
    Stdio(McpStdioServerConfig),
    #[serde(rename = "sse")]
    Sse(McpSseServerConfig),
    #[serde(rename = "http")]
    Http(McpHttpServerConfig),
    #[serde(rename = "sdk")]
    Sdk(McpSdkServerConfig),
}

/// Local plugin configuration supported by the SDK.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SdkPluginConfig {
    #[serde(rename = "type")]
    pub kind: SdkPluginKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SdkPluginKind {
    #[serde(rename = "local")]
    Local,
}

/// Representation of MCP server configuration input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum McpServers {
    Map(HashMap<String, McpServerConfig>),
    Path(PathBuf),
    Inline(String),
}

impl Default for McpServers {
    fn default() -> Self {
        Self::Map(HashMap::new())
    }
}

/// Callback invoked when the CLI writes to stderr.
pub type StderrCallback = Arc<dyn Fn(&str) + Send + Sync + 'static>;

/// Query options for Claude SDK.
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ClaudeAgentOptions {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<SystemPrompt>,
    pub mcp_servers: McpServers,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    pub continue_conversation: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disallowed_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_prompt_tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub add_dirs: Vec<PathBuf>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub extra_args: HashMap<String, Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_buffer_size: Option<usize>,
    #[serde(skip)]
    pub debug_stderr: Option<StderrCallback>,
    #[serde(skip)]
    pub stderr: Option<StderrCallback>,
    #[serde(skip)]
    pub can_use_tool: Option<CanUseToolHandle>,
    #[serde(skip)]
    pub hooks: Option<HashMap<HookEvent, Vec<HookMatcher>>>,
    #[serde(skip)]
    pub sdk_servers: HashMap<String, Arc<dyn SdkMcpServer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    pub include_partial_messages: bool,
    pub fork_session: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<HashMap<String, AgentDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setting_sources: Option<Vec<SettingSource>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<SdkPluginConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_thinking_tokens: Option<u32>,
}

/// Helper to convert permission suggestions to CLI payloads.
pub fn serialize_permission_updates(updates: &[PermissionUpdate]) -> Vec<Value> {
    updates
        .iter()
        .map(PermissionUpdate::to_control_payload)
        .collect()
}

impl ClaudeAgentOptions {
    /// Register an SDK MCP server instance that will be hosted in-process.
    pub fn add_sdk_server(&mut self, name: impl Into<String>, server: Arc<dyn SdkMcpServer>) {
        let name = name.into();
        self.sdk_servers.insert(name.clone(), server);

        let mut map = match std::mem::take(&mut self.mcp_servers) {
            McpServers::Map(map) => map,
            McpServers::Path(_) | McpServers::Inline(_) => HashMap::new(),
        };

        map.insert(
            name.clone(),
            McpServerConfig::Sdk(McpSdkServerConfig {
                kind: McpServerKind::Sdk,
                name,
                instance: None,
            }),
        );

        self.mcp_servers = McpServers::Map(map);
    }
}

impl std::fmt::Debug for ClaudeAgentOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeAgentOptions")
            .field("allowed_tools", &self.allowed_tools)
            .field("system_prompt", &self.system_prompt)
            .field("mcp_servers", &self.mcp_servers)
            .field("permission_mode", &self.permission_mode)
            .field("continue_conversation", &self.continue_conversation)
            .field("resume", &self.resume)
            .field("max_turns", &self.max_turns)
            .field("max_budget_usd", &self.max_budget_usd)
            .field("disallowed_tools", &self.disallowed_tools)
            .field("model", &self.model)
            .field(
                "permission_prompt_tool_name",
                &self.permission_prompt_tool_name,
            )
            .field("cwd", &self.cwd)
            .field("cli_path", &self.cli_path)
            .field("settings", &self.settings)
            .field("add_dirs", &self.add_dirs)
            .field("env", &self.env)
            .field("extra_args", &self.extra_args)
            .field("max_buffer_size", &self.max_buffer_size)
            .field("has_debug_stderr", &self.debug_stderr.is_some())
            .field("has_stderr", &self.stderr.is_some())
            .field("has_can_use_tool", &self.can_use_tool.is_some())
            .field("hooks_registered", &self.hooks.as_ref().map(|h| h.len()))
            .field("sdk_servers", &self.sdk_servers.len())
            .field("user", &self.user)
            .field("include_partial_messages", &self.include_partial_messages)
            .field("fork_session", &self.fork_session)
            .field("agents", &self.agents)
            .field("setting_sources", &self.setting_sources)
            .field("plugins", &self.plugins)
            .field("max_thinking_tokens", &self.max_thinking_tokens)
            .finish()
    }
}

//! Typed messages exchanged with the Claude Code CLI.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Text content block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextBlock {
    pub text: String,
}

/// Thinking content block produced by Claude.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThinkingBlock {
    pub thinking: String,
    pub signature: String,
}

/// Tool use content block describing a requested tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolUseBlock {
    pub id: String,
    pub name: String,
    pub input: Map<String, Value>,
}

/// Tool result block returned from a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultBlock {
    pub tool_use_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Union of all content blocks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text(TextBlock),
    #[serde(rename = "thinking")]
    Thinking(ThinkingBlock),
    #[serde(rename = "tool_use")]
    ToolUse(ToolUseBlock),
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultBlock),
}

/// Content for a user message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum UserMessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// User message emitted from the CLI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserMessage {
    pub content: UserMessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

/// Assistant message produced by Claude.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

/// System message containing metadata or warnings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemMessage {
    pub subtype: String,
    pub data: Map<String, Value>,
}

/// Result message summarising cost and usage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResultMessage {
    pub subtype: String,
    pub duration_ms: i64,
    pub duration_api_ms: i64,
    pub is_error: bool,
    pub num_turns: i64,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// Stream event for partial updates during streaming completions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamEvent {
    pub uuid: String,
    pub session_id: String,
    pub event: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

/// Messages emitted by the CLI.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
    System(SystemMessage),
    Result(ResultMessage),
    StreamEvent(StreamEvent),
}

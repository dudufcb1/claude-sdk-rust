//! Hook configuration and execution helpers.

use std::pin::Pin;
use std::sync::Arc;

use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Supported hook event names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookEvent {
    #[serde(rename = "PreToolUse")]
    PreToolUse,
    #[serde(rename = "PostToolUse")]
    PostToolUse,
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit,
    #[serde(rename = "Stop")]
    Stop,
    #[serde(rename = "SubagentStop")]
    SubagentStop,
    #[serde(rename = "PreCompact")]
    PreCompact,
}

impl HookEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::Stop => "Stop",
            HookEvent::SubagentStop => "SubagentStop",
            HookEvent::PreCompact => "PreCompact",
        }
    }
}

/// Base fields common to all hook inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseHookInput {
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
}

/// Input payload for the PreToolUse hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreToolUseHookInput {
    pub tool_name: String,
    pub tool_input: Map<String, Value>,
    #[serde(flatten)]
    pub base: BaseHookInput,
}

/// Input payload for the PostToolUse hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostToolUseHookInput {
    pub tool_name: String,
    pub tool_input: Map<String, Value>,
    pub tool_response: Value,
    #[serde(flatten)]
    pub base: BaseHookInput,
}

/// Input payload for the UserPromptSubmit hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPromptSubmitHookInput {
    pub prompt: String,
    #[serde(flatten)]
    pub base: BaseHookInput,
}

/// Input payload for the Stop hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopHookInput {
    pub stop_hook_active: bool,
    #[serde(flatten)]
    pub base: BaseHookInput,
}

/// Input payload for the SubagentStop hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentStopHookInput {
    pub stop_hook_active: bool,
    #[serde(flatten)]
    pub base: BaseHookInput,
}

/// Input payload for the PreCompact hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreCompactHookInput {
    pub trigger: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
    #[serde(flatten)]
    pub base: BaseHookInput,
}

/// Discriminated union of all hook inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "hookEventName")]
pub enum HookInput {
    #[serde(rename = "PreToolUse")]
    PreToolUse(PreToolUseHookInput),
    #[serde(rename = "PostToolUse")]
    PostToolUse(PostToolUseHookInput),
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit(UserPromptSubmitHookInput),
    #[serde(rename = "Stop")]
    Stop(StopHookInput),
    #[serde(rename = "SubagentStop")]
    SubagentStop(SubagentStopHookInput),
    #[serde(rename = "PreCompact")]
    PreCompact(PreCompactHookInput),
}

/// Hook-specific control output for PreToolUse events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PreToolUseHookSpecificOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Map<String, Value>>,
}

/// Hook-specific control output for PostToolUse events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PostToolUseHookSpecificOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

/// Hook-specific control output for UserPromptSubmit events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserPromptSubmitHookSpecificOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
}

/// Hook-specific output union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "hookEventName")]
pub enum HookSpecificOutput {
    #[serde(rename = "PreToolUse")]
    PreToolUse(PreToolUseHookSpecificOutput),
    #[serde(rename = "PostToolUse")]
    PostToolUse(PostToolUseHookSpecificOutput),
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit(UserPromptSubmitHookSpecificOutput),
}

/// Asynchronous hook response that defers execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsyncHookJsonOutput {
    #[serde(rename = "async")]
    pub is_async: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub async_timeout: Option<u64>,
}

/// Synchronous hook response payload.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncHookJsonOutput {
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub should_continue: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

/// Union of hook JSON outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HookJsonOutput {
    Async(AsyncHookJsonOutput),
    Sync(SyncHookJsonOutput),
}

/// Additional context passed to hook callbacks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HookContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<Value>,
}

/// Future returned by hook callbacks.
pub type HookCallbackFuture = Pin<Box<dyn Future<Output = HookJsonOutput> + Send>>;

/// Trait representing a hook callback implementation.
pub trait HookCallback: Send + Sync {
    fn call(
        &self,
        input: HookInput,
        tool_use_id: Option<String>,
        context: HookContext,
    ) -> HookCallbackFuture;
}

impl<F, Fut> HookCallback for F
where
    F: Fn(HookInput, Option<String>, HookContext) -> Fut + Send + Sync,
    Fut: Future<Output = HookJsonOutput> + Send + 'static,
{
    fn call(
        &self,
        input: HookInput,
        tool_use_id: Option<String>,
        context: HookContext,
    ) -> HookCallbackFuture {
        Box::pin(self(input, tool_use_id, context))
    }
}

/// Configuration binding a matcher description to hook callbacks.
#[derive(Clone)]
pub struct HookMatcher {
    pub matcher: Option<Value>,
    pub hooks: Vec<Arc<dyn HookCallback>>,
}

impl HookMatcher {
    pub fn new(matcher: Option<Value>) -> Self {
        Self {
            matcher,
            hooks: Vec::new(),
        }
    }
}

impl Default for HookMatcher {
    fn default() -> Self {
        Self::new(None)
    }
}

impl std::fmt::Debug for HookMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookMatcher")
            .field("matcher", &self.matcher)
            .field("hooks_len", &self.hooks.len())
            .finish()
    }
}

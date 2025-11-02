//! Permission handling types mirroring the Python SDK's permission system.

use std::pin::Pin;
use std::sync::Arc;

use futures::Future;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Permission mode requested from the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    Plan,
    BypassPermissions,
}

impl PermissionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionMode::Default => "default",
            PermissionMode::AcceptEdits => "acceptEdits",
            PermissionMode::Plan => "plan",
            PermissionMode::BypassPermissions => "bypassPermissions",
        }
    }
}

/// Destination for a permission update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionUpdateDestination {
    UserSettings,
    ProjectSettings,
    LocalSettings,
    Session,
}

impl PermissionUpdateDestination {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionUpdateDestination::UserSettings => "userSettings",
            PermissionUpdateDestination::ProjectSettings => "projectSettings",
            PermissionUpdateDestination::LocalSettings => "localSettings",
            PermissionUpdateDestination::Session => "session",
        }
    }
}

/// Behaviour for permission updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionBehavior {
    Allow,
    Deny,
    Ask,
}

impl PermissionBehavior {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionBehavior::Allow => "allow",
            PermissionBehavior::Deny => "deny",
            PermissionBehavior::Ask => "ask",
        }
    }
}

/// Individual permission rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRuleValue {
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_content: Option<String>,
}

impl PermissionRuleValue {
    pub fn new(tool_name: impl Into<String>, rule_content: Option<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            rule_content,
        }
    }
}

/// Type of permission update operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionUpdateKind {
    AddRules,
    ReplaceRules,
    RemoveRules,
    SetMode,
    AddDirectories,
    RemoveDirectories,
}

impl PermissionUpdateKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionUpdateKind::AddRules => "addRules",
            PermissionUpdateKind::ReplaceRules => "replaceRules",
            PermissionUpdateKind::RemoveRules => "removeRules",
            PermissionUpdateKind::SetMode => "setMode",
            PermissionUpdateKind::AddDirectories => "addDirectories",
            PermissionUpdateKind::RemoveDirectories => "removeDirectories",
        }
    }
}

/// Update payload sent back to the CLI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionUpdate {
    #[serde(rename = "type")]
    pub kind: PermissionUpdateKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<PermissionRuleValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior: Option<PermissionBehavior>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<PermissionMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directories: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<PermissionUpdateDestination>,
}

impl PermissionUpdate {
    pub fn new(kind: PermissionUpdateKind) -> Self {
        Self {
            kind,
            rules: None,
            behavior: None,
            mode: None,
            directories: None,
            destination: None,
        }
    }

    pub fn with_destination(mut self, destination: PermissionUpdateDestination) -> Self {
        self.destination = Some(destination);
        self
    }

    pub fn with_rules(mut self, rules: Vec<PermissionRuleValue>) -> Self {
        self.rules = Some(rules);
        self
    }

    pub fn with_behavior(mut self, behavior: PermissionBehavior) -> Self {
        self.behavior = Some(behavior);
        self
    }

    pub fn with_mode(mut self, mode: PermissionMode) -> Self {
        self.mode = Some(mode);
        self
    }

    pub fn with_directories(mut self, directories: Vec<String>) -> Self {
        self.directories = Some(directories);
        self
    }

    /// Convert the update into the control protocol JSON payload expected by the CLI.
    pub fn to_control_payload(&self) -> Value {
        serde_json::to_value(self).expect("PermissionUpdate should always serialize")
    }
}

/// Context passed to tool permission callbacks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPermissionContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<PermissionUpdate>,
}

/// Result variant for allowing a tool request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResultAllow {
    #[serde(default = "PermissionResultAllow::behavior_value")]
    pub behavior: PermissionBehavior,
    #[serde(rename = "updatedInput", skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Map<String, Value>>,
    #[serde(rename = "updatedPermissions", skip_serializing_if = "Option::is_none")]
    pub updated_permissions: Option<Vec<PermissionUpdate>>,
}

impl PermissionResultAllow {
    const fn behavior_value() -> PermissionBehavior {
        PermissionBehavior::Allow
    }
}

/// Result variant for denying a tool request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResultDeny {
    #[serde(default = "PermissionResultDeny::behavior_value")]
    pub behavior: PermissionBehavior,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default)]
    pub interrupt: bool,
}

impl PermissionResultDeny {
    const fn behavior_value() -> PermissionBehavior {
        PermissionBehavior::Deny
    }
}

/// Union of permission result variants.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "behavior", rename_all = "lowercase")]
pub enum PermissionResult {
    #[serde(rename = "allow")]
    Allow {
        #[serde(rename = "updatedInput", skip_serializing_if = "Option::is_none")]
        updated_input: Option<Map<String, Value>>,
        #[serde(rename = "updatedPermissions", skip_serializing_if = "Option::is_none")]
        updated_permissions: Option<Vec<PermissionUpdate>>,
    },
    #[serde(rename = "deny")]
    Deny {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        message: String,
        #[serde(default)]
        interrupt: bool,
    },
}

impl From<PermissionResultAllow> for PermissionResult {
    fn from(value: PermissionResultAllow) -> Self {
        PermissionResult::Allow {
            updated_input: value.updated_input,
            updated_permissions: value.updated_permissions,
        }
    }
}

impl From<PermissionResultDeny> for PermissionResult {
    fn from(value: PermissionResultDeny) -> Self {
        PermissionResult::Deny {
            message: value.message,
            interrupt: value.interrupt,
        }
    }
}

/// Boxed future returned by tool permission callbacks.
pub type ToolPermissionFuture = Pin<Box<dyn Future<Output = PermissionResult> + Send>>;

/// Signature for tool permission callbacks.
pub trait CanUseToolCallback: Send + Sync {
    fn call(
        &self,
        tool_name: &str,
        input: Map<String, Value>,
        context: ToolPermissionContext,
    ) -> ToolPermissionFuture;
}

impl<F, Fut> CanUseToolCallback for F
where
    F: Fn(&str, Map<String, Value>, ToolPermissionContext) -> Fut + Send + Sync,
    Fut: Future<Output = PermissionResult> + Send + 'static,
{
    fn call(
        &self,
        tool_name: &str,
        input: Map<String, Value>,
        context: ToolPermissionContext,
    ) -> ToolPermissionFuture {
        Box::pin(self(tool_name, input, context))
    }
}

/// Convenient handle for storing permission callbacks.
pub type CanUseToolHandle = Arc<dyn CanUseToolCallback>;

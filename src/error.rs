//! Error types exposed by the Rust SDK.

use std::path::PathBuf;

use serde_json::Value;
use thiserror::Error;

/// Top-level error type for all SDK operations.
#[derive(Debug, Error)]
pub enum SdkError {
    /// Placeholder for unimplemented functionality.
    #[error("not implemented")]
    NotImplemented,

    /// Generic error message.
    #[error("{0}")]
    Message(String),

    /// Raised when unable to connect to the Claude Code CLI.
    #[error(transparent)]
    CliConnection(#[from] CliConnectionError),

    /// Raised when the Claude Code CLI binary cannot be located.
    #[error(transparent)]
    CliNotFound(#[from] CliNotFoundError),

    /// Raised when the CLI process exits with an error.
    #[error(transparent)]
    Process(#[from] ProcessError),

    /// Raised when JSON output from the CLI cannot be decoded.
    #[error(transparent)]
    CliJsonDecode(#[from] CliJsonDecodeError),

    /// Raised when a CLI message cannot be parsed into a typed structure.
    #[error(transparent)]
    MessageParse(#[from] MessageParseError),

    /// IO error wrapper.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error wrapper.
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// Timeout while awaiting a CLI response.
    #[error(transparent)]
    Timeout(#[from] tokio::time::error::Elapsed),
}

/// Raised when unable to connect to the Claude Code CLI.
#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct CliConnectionError {
    message: String,
}

impl CliConnectionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Raised when Claude Code is not found or not installed.
#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct CliNotFoundError {
    message: String,
    cli_path: Option<PathBuf>,
}

impl CliNotFoundError {
    pub fn new(message: impl Into<String>, cli_path: Option<PathBuf>) -> Self {
        let message = match cli_path.as_ref() {
            Some(path) => format!("{}: {}", message.into(), path.display()),
            None => message.into(),
        };
        Self { message, cli_path }
    }

    pub fn cli_path(&self) -> Option<&PathBuf> {
        self.cli_path.as_ref()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Raised when the CLI process fails.
#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct ProcessError {
    message: String,
    exit_code: Option<i32>,
    stderr: Option<String>,
}

impl ProcessError {
    pub fn new(message: impl Into<String>, exit_code: Option<i32>, stderr: Option<String>) -> Self {
        let mut message = message.into();

        if let Some(code) = exit_code {
            message = format!("{} (exit code: {})", message, code);
        }

        if let Some(ref stderr) = stderr {
            if !stderr.is_empty() {
                message = format!("{message}\nError output: {stderr}");
            }
        }

        Self {
            message,
            exit_code,
            stderr,
        }
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub fn stderr(&self) -> Option<&str> {
        self.stderr.as_deref()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Raised when JSON output from the CLI cannot be decoded.
#[derive(Debug, Error)]
#[error("Failed to decode JSON: {snippet}...")]
pub struct CliJsonDecodeError {
    line: String,
    #[source]
    source: serde_json::Error,
    snippet: String,
}

impl CliJsonDecodeError {
    pub fn new(line: impl Into<String>, source: serde_json::Error) -> Self {
        let line = line.into();
        let snippet = line.chars().take(100).collect::<String>();
        Self {
            line,
            source,
            snippet,
        }
    }

    pub fn line(&self) -> &str {
        &self.line
    }
}

/// Raised when a CLI message cannot be parsed into a typed structure.
#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct MessageParseError {
    message: String,
    data: Option<Value>,
}

impl MessageParseError {
    pub fn new(message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            message: message.into(),
            data,
        }
    }

    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cli_connection_error_preserves_message() {
        let err = CliConnectionError::new("Failed to connect");
        assert_eq!(err.message(), "Failed to connect");
        assert_eq!(err.to_string(), "Failed to connect");
    }

    #[test]
    fn cli_not_found_error_formats_path_when_available() {
        let err = CliNotFoundError::new("Claude CLI not found", Some(PathBuf::from("/tmp/claude")));
        assert!(err.message().contains("Claude CLI not found"));
        assert!(err.message().contains("/tmp/claude"));
    }

    #[test]
    fn process_error_includes_exit_code_and_stderr() {
        let err = ProcessError::new("Process failed", Some(1), Some("Command not found".into()));
        assert_eq!(err.exit_code(), Some(1));
        assert_eq!(err.stderr(), Some("Command not found"));
        let message = err.message();
        assert!(message.contains("Process failed"));
        assert!(message.contains("exit code: 1"));
        assert!(message.contains("Command not found"));
    }

    #[test]
    fn cli_json_decode_error_exposes_line_and_message() {
        let source = serde_json::from_str::<serde_json::Value>("{invalid json}").unwrap_err();
        let err = CliJsonDecodeError::new("{invalid json}", source);
        assert_eq!(err.line(), "{invalid json}");
        assert!(err.to_string().contains("Failed to decode JSON"));
    }

    #[test]
    fn message_parse_error_retains_payload() {
        let payload = json!({"type": "unknown"});
        let err = MessageParseError::new("unknown type", Some(payload.clone()));
        assert_eq!(err.message(), "unknown type");
        assert_eq!(err.data(), Some(&payload));
    }
}

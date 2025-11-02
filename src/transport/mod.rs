//! Transport abstraction used by the SDK.

/// Trait representing a bidirectional channel to the Claude Code CLI.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Connect to the underlying channel.
    async fn connect(&self) -> Result<(), crate::error::SdkError>;

    /// Write a raw JSON message to the CLI.
    async fn write(&self, payload: &serde_json::Value) -> Result<(), crate::error::SdkError>;

    /// Read the next JSON message produced by the CLI.
    async fn read(&self) -> Result<Option<serde_json::Value>, crate::error::SdkError>;

    /// Finish sending input to the CLI.
    async fn end_input(&self) -> Result<(), crate::error::SdkError>;

    /// Close the transport.
    async fn close(&self) -> Result<(), crate::error::SdkError>;

    /// Whether the transport is ready for IO.
    fn is_ready(&self) -> bool;
}

pub mod subprocess_cli;

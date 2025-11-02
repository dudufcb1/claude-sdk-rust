//! High-level client API for interacting with the Claude Code CLI.

use std::sync::Arc;

use futures::stream::BoxStream;
use futures::{stream, Stream, StreamExt};
use serde_json::{json, Value};
use tokio::task::JoinHandle;

use crate::config::ClaudeAgentOptions;
use crate::error::{CliConnectionError, SdkError};
use crate::internal::client::PromptInput;
use crate::internal::query::Query;
use crate::message::Message;
use crate::permission::PermissionMode;
use crate::transport::subprocess_cli::{PromptMode, SubprocessCliTransport};
use crate::transport::Transport;

/// Convenience alias for trait-object transports.
pub type DynTransport = Arc<dyn Transport>;

/// Public client surface matching the Python SDK behaviour.
pub struct ClaudeSdkClient {
    options: ClaudeAgentOptions,
    custom_transport: Option<DynTransport>,
    transport: Option<DynTransport>,
    query: Option<Query<dyn Transport>>, // Query already wraps Arc internally
    prompt_task: Option<JoinHandle<()>>,
    server_info: Option<Value>,
    connected: bool,
}

impl Default for ClaudeSdkClient {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl ClaudeSdkClient {
    /// Create a new client with optional configuration and transport override.
    pub fn new(options: Option<ClaudeAgentOptions>, transport: Option<DynTransport>) -> Self {
        std::env::set_var("CLAUDE_CODE_ENTRYPOINT", "sdk-rs-client");
        Self {
            options: options.unwrap_or_default(),
            custom_transport: transport,
            transport: None,
            query: None,
            prompt_task: None,
            server_info: None,
            connected: false,
        }
    }

    /// Connect to Claude Code with an optional initial prompt stream.
    pub async fn connect(&mut self, prompt: Option<PromptInput>) -> Result<(), SdkError> {
        if self.connected {
            return Ok(());
        }

        let prompt =
            prompt.unwrap_or_else(|| PromptInput::from_stream(tokio_stream::empty::<Value>()));
        let is_streaming = prompt.is_streaming();

        Self::validate_permission_options(&mut self.options, is_streaming)?;

        let (prompt_mode, stream_source) = match prompt {
            PromptInput::Text(text) => (PromptMode::Text(text), None),
            PromptInput::Stream(stream) => (PromptMode::Streaming, Some(stream)),
        };

        let transport: DynTransport = if let Some(custom) = &self.custom_transport {
            Arc::clone(custom)
        } else {
            let transport_options = self.options.clone();
            let subprocess = SubprocessCliTransport::new(prompt_mode, transport_options)?;
            Arc::new(subprocess)
        };

        transport.connect().await?;

        let query = Query::new(
            Arc::clone(&transport),
            true,
            self.options.can_use_tool.clone(),
            self.options.hooks.clone(),
            self.options.sdk_servers.clone(),
        );

        query.start().await?;
        self.server_info = query.initialize().await?;

        if let Some(stream) = stream_source {
            let query_clone = query.clone();
            self.prompt_task = Some(tokio::spawn(async move {
                if let Err(_err) = query_clone.stream_input(stream).await {
                    let _ = query_clone.close().await;
                }
            }));
        }

        self.transport = Some(transport);
        self.query = Some(query);
        self.connected = true;
        Ok(())
    }

    /// Receive all messages yielded by the current query session.
    pub fn receive_messages(
        &self,
    ) -> Result<impl Stream<Item = Result<Message, SdkError>>, SdkError> {
        let query = self
            .query
            .as_ref()
            .ok_or_else(|| CliConnectionError::new("Not connected"))?
            .clone();

        Ok(Self::message_stream(query))
    }

    /// Receive messages until the first [`ResultMessage`] inclusive.
    pub fn receive_response(
        &self,
    ) -> Result<impl Stream<Item = Result<Message, SdkError>>, SdkError> {
        let query = self
            .query
            .as_ref()
            .ok_or_else(|| CliConnectionError::new("Not connected"))?
            .clone();
        Ok(Self::response_stream(query))
    }

    /// Send a new request in streaming mode.
    pub async fn query<Q>(&self, prompt: Q, session_id: &str) -> Result<(), SdkError>
    where
        Q: Into<ClientPrompt>,
    {
        let prompt = prompt.into();
        let transport = self
            .transport
            .as_ref()
            .ok_or_else(|| CliConnectionError::new("Not connected"))?;
        if self.query.is_none() {
            return Err(CliConnectionError::new("Not connected").into());
        }

        match prompt {
            ClientPrompt::Text(text) => {
                let message = json!({
                    "type": "user",
                    "message": { "role": "user", "content": text },
                    "parent_tool_use_id": Value::Null,
                    "session_id": session_id,
                });
                transport.write(&message).await?
            }
            ClientPrompt::Stream(mut stream) => {
                while let Some(mut value) = stream.next().await {
                    if value.get("session_id").is_none() {
                        value["session_id"] = Value::String(session_id.to_string());
                    }
                    transport.write(&value).await?;
                }
            }
        }

        Ok(())
    }

    /// Interrupt the current conversation.
    pub async fn interrupt(&self) -> Result<(), SdkError> {
        let query = self
            .query
            .as_ref()
            .ok_or_else(|| CliConnectionError::new("Not connected"))?;
        query.interrupt().await
    }

    /// Update the permission mode during an active session.
    pub async fn set_permission_mode(&mut self, mode: PermissionMode) -> Result<(), SdkError> {
        let query = self
            .query
            .as_ref()
            .ok_or_else(|| CliConnectionError::new("Not connected"))?;
        query.set_permission_mode(mode).await?;
        self.options.permission_mode = Some(mode);
        Ok(())
    }

    /// Update the active model during an active session.
    pub async fn set_model(&mut self, model: Option<String>) -> Result<(), SdkError> {
        let query = self
            .query
            .as_ref()
            .ok_or_else(|| CliConnectionError::new("Not connected"))?;
        query.set_model(model.clone()).await?;
        self.options.model = model;
        Ok(())
    }

    /// Get initialization metadata returned by the server.
    pub fn get_server_info(&self) -> Option<Value> {
        self.server_info.clone()
    }

    /// Disconnect and release transport resources.
    pub async fn disconnect(&mut self) -> Result<(), SdkError> {
        if let Some(handle) = self.prompt_task.take() {
            handle.abort();
            let _ = handle.await;
        }

        if let Some(query) = self.query.take() {
            query.close().await?;
        }

        self.transport = None;
        self.server_info = None;
        self.connected = false;
        Ok(())
    }

    fn message_stream<T>(query: Query<T>) -> impl Stream<Item = Result<Message, SdkError>>
    where
        T: Transport + ?Sized + 'static,
    {
        stream::unfold((query, false), |(query, finished)| async move {
            if finished {
                return None;
            }

            match query.next_message().await {
                Ok(Some(message)) => Some((Ok(message), (query, false))),
                Ok(None) => {
                    let _ = query.close().await;
                    None
                }
                Err(err) => {
                    let _ = query.close().await;
                    Some((Err(err), (query, true)))
                }
            }
        })
    }

    fn response_stream<T>(query: Query<T>) -> impl Stream<Item = Result<Message, SdkError>>
    where
        T: Transport + ?Sized + 'static,
    {
        stream::unfold((query, false), |(query, finished)| async move {
            if finished {
                return None;
            }

            match query.next_message().await {
                Ok(Some(message)) => {
                    let done = matches!(message, Message::Result(_));
                    Some((Ok(message), (query, done)))
                }
                Ok(None) => {
                    let _ = query.close().await;
                    None
                }
                Err(err) => {
                    let _ = query.close().await;
                    Some((Err(err), (query, true)))
                }
            }
        })
    }

    fn validate_permission_options(
        options: &mut ClaudeAgentOptions,
        is_streaming: bool,
    ) -> Result<(), SdkError> {
        if options.can_use_tool.is_some() {
            if !is_streaming {
                return Err(SdkError::Message(
                    "can_use_tool callback requires streaming mode".into(),
                ));
            }

            if options.permission_prompt_tool_name.is_some() {
                return Err(SdkError::Message(
                    "can_use_tool cannot be used with permission_prompt_tool_name".into(),
                ));
            }

            options.permission_prompt_tool_name = Some("stdio".into());
        }
        Ok(())
    }
}

/// Inputs accepted by [`ClaudeSdkClient::query`].
pub enum ClientPrompt {
    Text(String),
    Stream(BoxStream<'static, Value>),
}

impl ClientPrompt {
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = Value> + Send + 'static,
    {
        ClientPrompt::Stream(stream.boxed())
    }
}

impl From<&str> for ClientPrompt {
    fn from(value: &str) -> Self {
        ClientPrompt::Text(value.to_string())
    }
}

impl From<String> for ClientPrompt {
    fn from(value: String) -> Self {
        ClientPrompt::Text(value)
    }
}

impl From<BoxStream<'static, Value>> for ClientPrompt {
    fn from(stream: BoxStream<'static, Value>) -> Self {
        ClientPrompt::Stream(stream)
    }
}

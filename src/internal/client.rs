//! Internal client used by the public query helper and one-shot API.

use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::BoxStream;
use futures::{stream, Stream, StreamExt};
use serde_json::{Map, Value};

use crate::config::ClaudeAgentOptions;
use crate::error::SdkError;
use crate::hooks::{HookEvent, HookMatcher};
use crate::internal::query::Query;
use crate::message::Message;
use crate::transport::subprocess_cli::{PromptMode, SubprocessCliTransport};
use crate::transport::Transport;

/// Prompt input accepted by the internal client.
pub enum PromptInput {
    Text(String),
    Stream(BoxStream<'static, Value>),
}

impl PromptInput {
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = Value> + Send + 'static,
    {
        PromptInput::Stream(stream.boxed())
    }

    pub fn is_streaming(&self) -> bool {
        matches!(self, PromptInput::Stream(_))
    }
}

impl From<String> for PromptInput {
    fn from(value: String) -> Self {
        PromptInput::Text(value)
    }
}

impl From<&str> for PromptInput {
    fn from(value: &str) -> Self {
        PromptInput::Text(value.to_string())
    }
}

impl From<BoxStream<'static, Value>> for PromptInput {
    fn from(stream: BoxStream<'static, Value>) -> Self {
        PromptInput::Stream(stream)
    }
}

/// Internal helper that mirrors the Python `_internal.client` module.
#[derive(Debug, Default)]
pub struct InternalClient;

impl InternalClient {
    /// Create a new internal client.
    pub fn new() -> Self {
        Self
    }

    /// Process a query through the transport and control protocol, returning a message stream.
    pub async fn process_query(
        &self,
        prompt: PromptInput,
        mut options: ClaudeAgentOptions,
        transport: Option<Arc<dyn Transport>>,
    ) -> Result<impl Stream<Item = Result<Message, SdkError>>, SdkError> {
        let is_streaming = prompt.is_streaming();
        Self::validate_permission_options(&mut options, is_streaming)?;

        let (prompt_mode, stream_source) = match prompt {
            PromptInput::Text(text) => (PromptMode::Text(text), None),
            PromptInput::Stream(stream) => (PromptMode::Streaming, Some(stream)),
        };

        let transport = if let Some(custom) = transport {
            custom
        } else {
            let transport_options = options.clone();
            let subprocess = SubprocessCliTransport::new(prompt_mode, transport_options)?;
            Arc::new(subprocess) as Arc<dyn Transport>
        };

        transport.connect().await?;

        let hooks = options.hooks.clone();
        let sdk_servers = options.sdk_servers.clone();
        let can_use_tool = options.can_use_tool.clone();

        let query: Query<dyn Transport> = Query::new(
            Arc::clone(&transport),
            is_streaming,
            can_use_tool,
            hooks,
            sdk_servers,
        );

        query.start().await?;

        if is_streaming {
            query.initialize().await?;
        }

        if let Some(stream) = stream_source {
            let query_clone = query.clone();
            tokio::spawn(async move {
                if let Err(err) = query_clone.stream_input(stream).await {
                    let _ = query_clone.close().await;
                    let _ = err;
                }
            });
        }

        Ok(Self::message_stream(query))
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

    fn validate_permission_options(
        options: &mut ClaudeAgentOptions,
        is_streaming: bool,
    ) -> Result<(), SdkError> {
        if options.can_use_tool.is_some() {
            if !is_streaming {
                return Err(SdkError::Message(
                    "can_use_tool callback requires streaming prompt".into(),
                ));
            }

            if options.permission_prompt_tool_name.is_some() {
                return Err(SdkError::Message(
                    "can_use_tool cannot be combined with permission_prompt_tool_name".into(),
                ));
            }

            options.permission_prompt_tool_name = Some("stdio".into());
        }

        Ok(())
    }

    #[allow(dead_code)]
    fn convert_hooks_to_internal_format(
        hooks: &HashMap<HookEvent, Vec<HookMatcher>>,
    ) -> HashMap<String, Vec<Map<String, Value>>> {
        let mut internal: HashMap<String, Vec<Map<String, Value>>> = HashMap::new();
        for (event, matchers) in hooks {
            let entries = internal.entry(event.as_str().to_string()).or_default();
            for matcher in matchers {
                let mut entry = Map::new();
                if let Some(matcher_value) = matcher.matcher.clone() {
                    entry.insert("matcher".into(), matcher_value);
                }
                entry.insert("hooks".into(), Value::Null); // placeholder; Query handles actual callbacks
                entries.push(entry);
            }
        }
        internal
    }
}

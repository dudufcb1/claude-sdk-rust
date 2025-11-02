//! One-shot query helper mirroring the Python `query` coroutine.

use futures::Stream;

use crate::client::DynTransport;
use crate::config::ClaudeAgentOptions;
use crate::error::SdkError;
use crate::internal::client::{InternalClient, PromptInput};
use crate::message::Message;

/// Execute a one-off query against Claude Code, yielding streamed messages.
pub async fn query<P>(
    prompt: P,
    options: Option<ClaudeAgentOptions>,
    transport: Option<DynTransport>,
) -> Result<impl Stream<Item = Result<Message, SdkError>>, SdkError>
where
    P: Into<PromptInput>,
{
    std::env::set_var("CLAUDE_CODE_ENTRYPOINT", "sdk-rs");

    let internal = InternalClient::new();
    let prompt = prompt.into();
    let options = options.unwrap_or_default();

    internal.process_query(prompt, options, transport).await
}

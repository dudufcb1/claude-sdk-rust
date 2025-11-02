use std::collections::HashMap;
use std::sync::Arc;

use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::ClaudeAgentOptions;
use sdk_claude_rust::hooks::{
    HookEvent, HookInput, HookJsonOutput, HookMatcher, SyncHookJsonOutput,
};
use sdk_claude_rust::message::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut matcher = HookMatcher::default();
    matcher
        .hooks
        .push(Arc::new(|input, tool_use_id, _| async move {
            if let HookInput::PreToolUse(payload) = input {
                println!(
                    "Hook intercepted tool {} with input {:?}",
                    payload.tool_name, payload.tool_input
                );
                if let Some(id) = tool_use_id {
                    println!("tool_use_id: {id}");
                }
            }
            HookJsonOutput::Sync(SyncHookJsonOutput::default())
        }));

    let mut hooks = HashMap::new();
    hooks.insert(HookEvent::PreToolUse, vec![matcher]);
    let options = ClaudeAgentOptions {
        hooks: Some(hooks),
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;

    client
        .query(
            "Use a tool to display the current working directory.",
            "hooks-session",
        )
        .await?;

    let stream = client.receive_messages()?;
    pin_mut!(stream);
    while let Some(message) = stream.next().await {
        println!("{message:?}");
        if matches!(message, Ok(Message::Result(_))) {
            break;
        }
    }

    client.disconnect().await?;
    Ok(())
}

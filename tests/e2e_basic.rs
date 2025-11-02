use std::time::Duration;

use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::ClaudeAgentOptions;
use sdk_claude_rust::internal::client::PromptInput;
use sdk_claude_rust::message::Message;
use sdk_claude_rust::permission::PermissionMode;

fn ensure_api_key() -> bool {
    std::env::var("ANTHROPIC_API_KEY").is_ok()
}

#[tokio::test]
#[ignore = "Requires Claude CLI installed and ANTHROPIC_API_KEY set"]
async fn e2e_agents_and_settings_flow() -> Result<(), Box<dyn std::error::Error>> {
    if !ensure_api_key() {
        eprintln!("Skipping e2e_agents_and_settings_flow: ANTHROPIC_API_KEY not set");
        return Ok(());
    }

    let options = ClaudeAgentOptions {
        permission_mode: Some(PermissionMode::AcceptEdits),
        include_partial_messages: true,
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client
        .connect(Some(PromptInput::from("List three rustfmt tips")))
        .await?;

    let stream = client.receive_response()?;
    pin_mut!(stream);
    let mut assistant_seen = false;
    let mut result_seen = false;

    while let Some(message) = stream.next().await {
        match message? {
            Message::Assistant(_) => assistant_seen = true,
            Message::Result(result) => {
                result_seen = true;
                assert!(!result.session_id.is_empty());
                break;
            }
            _ => {}
        }
    }

    assert!(assistant_seen, "expected an assistant message in e2e flow");
    assert!(result_seen, "expected a result message in e2e flow");

    client.disconnect().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Claude CLI installed and ANTHROPIC_API_KEY set"]
async fn e2e_streaming_interrupt_flow() -> Result<(), Box<dyn std::error::Error>> {
    if !ensure_api_key() {
        eprintln!("Skipping e2e_streaming_interrupt_flow: ANTHROPIC_API_KEY not set");
        return Ok(());
    }

    use futures::stream;
    use serde_json::json;

    let options = ClaudeAgentOptions {
        permission_mode: Some(PermissionMode::Plan),
        ..Default::default()
    };

    let prompt_stream = stream::iter(vec![json!({
        "type": "user",
        "message": {"role": "user", "content": [
            {"type": "text", "text": "Explain the borrow checker in detail"}
        ]},
    })]);

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client
        .connect(Some(PromptInput::from_stream(prompt_stream)))
        .await?;

    // Allow the stream to start delivering tokens before issuing interrupt.
    tokio::time::sleep(Duration::from_secs(2)).await;
    client.interrupt().await?;

    let stream = client.receive_messages()?;
    pin_mut!(stream);
    let mut result_seen = false;
    while let Some(message) = stream.next().await {
        if let Message::Result(result) = message? {
            result_seen = true;
            assert!(result.is_error || result.subtype == "interrupted");
            break;
        }
    }

    assert!(result_seen, "expected a result message after interrupt");
    client.disconnect().await?;
    Ok(())
}

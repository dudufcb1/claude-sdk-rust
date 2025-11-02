use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::{ClaudeAgentOptions, SystemPrompt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = ClaudeAgentOptions {
        system_prompt: Some(SystemPrompt::Text(
            "You are an extremely concise assistant. Always reply with one sentence.".into(),
        )),
        model: Some("claude-sonnet-4-5".into()),
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;
    client
        .query("Explain Rust ownership in one sentence.", "system-prompt")
        .await?;

    let stream = client.receive_response()?;
    pin_mut!(stream);
    while let Some(message) = stream.next().await {
        println!("{message:?}");
    }

    client.disconnect().await?;
    Ok(())
}

use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::ClaudeAgentOptions;
use sdk_claude_rust::message::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = ClaudeAgentOptions {
        include_partial_messages: true,
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;

    client
        .query(
            "Summarise the Rust ownership model in a few sentences.",
            "partial",
        )
        .await?;

    let stream = client.receive_messages()?;
    pin_mut!(stream);
    while let Some(message) = stream.next().await {
        match message? {
            Message::StreamEvent(event) => {
                println!("stream event: {}", event.uuid);
            }
            other => println!("{other:?}"),
        }
    }

    client.disconnect().await?;
    Ok(())
}

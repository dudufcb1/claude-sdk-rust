use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::ClaudeAgentOptions;
use sdk_claude_rust::message::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = ClaudeAgentOptions {
        max_budget_usd: Some(0.10),
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;

    client
        .query("Write a haiku about budgeting resources.", "budget")
        .await?;

    let stream = client.receive_response()?;
    pin_mut!(stream);
    while let Some(message) = stream.next().await {
        match message? {
            Message::Result(result) => {
                println!(
                    "Session {} consumed cost {:?}",
                    result.session_id, result.total_cost_usd
                );
            }
            other => println!("{other:?}"),
        }
    }

    client.disconnect().await?;
    Ok(())
}

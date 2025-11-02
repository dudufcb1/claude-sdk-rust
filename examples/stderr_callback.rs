use std::sync::Arc;

use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::{ClaudeAgentOptions, StderrCallback};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let callback: StderrCallback = Arc::new(|line| {
        eprintln!("[stderr] {line}");
    });
    let options = ClaudeAgentOptions {
        stderr: Some(callback),
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;

    client
        .query("Run a bash command that prints to stderr", "stderr-session")
        .await?;

    let stream = client.receive_response()?;
    pin_mut!(stream);
    while let Some(message) = stream.next().await {
        println!("{message:?}");
    }

    client.disconnect().await?;
    Ok(())
}

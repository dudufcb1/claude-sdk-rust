use std::path::PathBuf;

use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::{ClaudeAgentOptions, SdkPluginConfig, SdkPluginKind};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = ClaudeAgentOptions {
        plugins: vec![SdkPluginConfig {
            kind: SdkPluginKind::Local,
            path: PathBuf::from("examples/plugins/demo-plugin"),
        }],
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;

    client
        .query(
            "Use the demo plugin greet command to welcome the user.",
            "plugin-demo",
        )
        .await?;

    let stream = client.receive_response()?;
    pin_mut!(stream);
    while let Some(message) = stream.next().await {
        println!("{message:?}");
    }

    client.disconnect().await?;
    Ok(())
}

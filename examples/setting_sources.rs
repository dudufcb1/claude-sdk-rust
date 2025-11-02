use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::{ClaudeAgentOptions, SettingSource};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = ClaudeAgentOptions {
        setting_sources: Some(vec![SettingSource::Project, SettingSource::User]),
        settings: Some("project-settings.json".into()),
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;

    client
        .query(
            "Explain how settings sources can influence behaviour.",
            "settings",
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

use std::sync::Arc;

use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::ClaudeAgentOptions;
use sdk_claude_rust::permission::{PermissionResult, ToolPermissionContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let callback = Arc::new(
        |tool_name: &str,
         input: serde_json::Map<String, serde_json::Value>,
         _ctx: ToolPermissionContext| {
            println!("Permission requested for tool {tool_name} with input {input:?}");
            Box::pin(async move {
                PermissionResult::Allow {
                    updated_input: None,
                    updated_permissions: None,
                }
            })
        },
    );
    let options = ClaudeAgentOptions {
        can_use_tool: Some(callback),
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;

    client
        .query("Run a quick ls command", "permission-session")
        .await?;

    let stream = client.receive_response()?;
    pin_mut!(stream);
    while let Some(message) = stream.next().await {
        println!("{message:?}");
    }

    client.disconnect().await?;
    Ok(())
}

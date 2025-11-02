use futures::{pin_mut, StreamExt};
use serde_json::{json, Map, Value};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::ClaudeAgentOptions;
use sdk_claude_rust::mcp::{create_sdk_mcp_server, tool, McpToolCallResult, McpToolContent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let add_tool = tool(
        "calculator.add",
        "Add two floating point numbers",
        json!({
            "type": "object",
            "properties": {
                "a": {"type": "number"},
                "b": {"type": "number"}
            },
            "required": ["a", "b"]
        }),
        |mut args: Map<String, Value>| async move {
            let a = args
                .remove("a")
                .and_then(|value| value.as_f64())
                .ok_or_else(|| sdk_claude_rust::error::SdkError::Message("missing 'a'".into()))?;
            let b = args
                .remove("b")
                .and_then(|value| value.as_f64())
                .ok_or_else(|| sdk_claude_rust::error::SdkError::Message("missing 'b'".into()))?;
            let sum = a + b;
            Ok(McpToolCallResult::new(vec![McpToolContent::text(format!(
                "Result: {:.3}",
                sum
            ))]))
        },
    );

    let server = create_sdk_mcp_server("calculator", "0.1.0", vec![add_tool]);

    let mut options = ClaudeAgentOptions::default();
    options.add_sdk_server("calculator", server);

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;

    client
        .query(
            "Call calculator.add with a=1.5 and b=2.25 and report the result.",
            "mcp-demo",
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

use std::collections::HashMap;

use futures::{pin_mut, StreamExt};

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::{AgentDefinition, ClaudeAgentOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut agents = HashMap::new();
    agents.insert(
        "planner".to_string(),
        AgentDefinition {
            description: "High level planner".into(),
            prompt: "Break tasks into concise steps.".into(),
            tools: None,
            model: Some("claude-3-5-sonnet-latest".into()),
        },
    );
    agents.insert(
        "executor".to_string(),
        AgentDefinition {
            description: "Executes bash commands".into(),
            prompt: "Run the provided shell commands carefully.".into(),
            tools: Some(vec!["Bash".into(), "Search".into()]),
            model: None,
        },
    );
    let options = ClaudeAgentOptions {
        agents: Some(agents),
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;
    client
        .query(
            "Use the agents you have to plan a mini todo list for learning Rust.",
            "agent-session",
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

use std::error::Error;

use futures::{pin_mut, StreamExt};
use sdk_claude_rust::config::ClaudeAgentOptions;
use sdk_claude_rust::message::{ContentBlock, Message, UserMessageContent};
use sdk_claude_rust::query::query;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        eprintln!("Set ANTHROPIC_API_KEY before running this example.");
        std::process::exit(1);
    }

    let stream = query(
        "List three reasons to build a Rust SDK for Claude Code.",
        Some(ClaudeAgentOptions::default()),
        None,
    )
    .await?;

    pin_mut!(stream);

    while let Some(message) = stream.next().await {
        match message? {
            Message::Assistant(assistant) => {
                for block in assistant.content {
                    if let ContentBlock::Text(text) = block {
                        println!("Assistant: {}", text.text);
                    }
                }
            }
            Message::User(user) => match user.content {
                UserMessageContent::Text(text) => println!("User: {}", text),
                UserMessageContent::Blocks(blocks) => {
                    for block in blocks {
                        if let ContentBlock::Text(text) = block {
                            println!("User: {}", text.text);
                        }
                    }
                }
            },
            Message::System(system) => {
                println!("System [{}]: {:?}", system.subtype, system.data);
            }
            Message::Result(result) => {
                println!(
                    "Result: {} (turns: {}, error: {})",
                    result.subtype, result.num_turns, result.is_error
                );
            }
            Message::StreamEvent(event) => {
                println!("Stream event: {:?}", event.event);
            }
        }
    }

    Ok(())
}

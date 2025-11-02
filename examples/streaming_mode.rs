//! Streaming mode examples mirroring the Python SDK demos.

use std::time::Duration;

use futures::StreamExt;
use tokio::time::sleep;

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::message::{ContentBlock, Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Streaming mode examples ===\n");

    basic_streaming_example().await?;
    multi_turn_example().await?;
    manual_message_handling().await?;
    interrupt_example().await?;

    Ok(())
}

async fn basic_streaming_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("-- basic stream --");
    let mut client = ClaudeSdkClient::default();
    client.connect(None).await?;

    client.query("What is 2 + 2?", "basic-stream").await?;

    let stream = client.receive_response()?;
    let mut stream = Box::pin(stream);
    while let Some(message) = stream.as_mut().next().await {
        display_message(message?);
    }

    client.disconnect().await?;
    println!();
    Ok(())
}

async fn multi_turn_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("-- multi turn --");
    let mut client = ClaudeSdkClient::default();
    client.connect(None).await?;

    client
        .query("What's the capital of France?", "multi-turn")
        .await?;
    let first = client.receive_response()?;
    let mut first = Box::pin(first);
    while let Some(message) = first.as_mut().next().await {
        display_message(message?);
    }

    client
        .query("What's the population of that city?", "multi-turn")
        .await?;
    let second = client.receive_response()?;
    let mut second = Box::pin(second);
    while let Some(message) = second.as_mut().next().await {
        display_message(message?);
    }

    client.disconnect().await?;
    println!();
    Ok(())
}

async fn manual_message_handling() -> Result<(), Box<dyn std::error::Error>> {
    println!("-- manual message handling --");
    let mut client = ClaudeSdkClient::default();
    client.connect(None).await?;

    client
        .query(
            "List five programming languages and a primary use case for each.",
            "manual",
        )
        .await?;

    let stream = client.receive_messages()?;
    let mut stream = Box::pin(stream);
    while let Some(message) = stream.as_mut().next().await {
        let message = message?;
        display_message(message.clone());
        if matches!(message, Message::Result(_)) {
            break;
        }
    }

    client.disconnect().await?;
    println!();
    Ok(())
}

async fn interrupt_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("-- interrupt --");
    println!("(requires running receive_messages to propagate interrupt)");

    let mut client = ClaudeSdkClient::default();
    client.connect(None).await?;

    client
        .query(
            "Count slowly from 1 upwards, pausing briefly between numbers.",
            "interrupt-session",
        )
        .await?;

    let stream = client.receive_messages()?;
    let receiver = tokio::spawn(async move {
        let mut stream = Box::pin(stream);
        while let Some(message) = stream.as_mut().next().await {
            if let Ok(msg) = message {
                display_message(msg.clone());
                if matches!(msg, Message::Result(_)) {
                    break;
                }
            }
        }
    });

    sleep(Duration::from_secs(2)).await;
    client.interrupt().await?;
    let _ = receiver.await;

    client
        .query(
            "Thanks, please tell me a short joke instead.",
            "interrupt-session",
        )
        .await?;
    let follow_up = client.receive_response()?;
    let mut follow_up = Box::pin(follow_up);
    while let Some(message) = follow_up.as_mut().next().await {
        display_message(message?);
    }

    client.disconnect().await?;
    println!();
    Ok(())
}

fn display_message(message: Message) {
    match message {
        Message::Assistant(assistant) => {
            for block in assistant.content {
                if let ContentBlock::Text(text) = block {
                    println!("Claude: {}", text.text);
                }
            }
        }
        Message::User(user) => {
            println!("User message: {:?}", user.parent_tool_use_id);
        }
        Message::Result(result) => {
            println!("Result subtype: {}", result.subtype);
            if let Some(cost) = result.total_cost_usd {
                println!("Total cost: ${cost:.4}");
            }
        }
        Message::System(system) => {
            println!("System event: {}", system.subtype);
        }
        Message::StreamEvent(_) => {
            println!("(stream event)");
        }
    }
}

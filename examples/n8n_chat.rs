use std::error::Error;
use std::io::{self, Write};

use futures::StreamExt;

use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::config::{ClaudeAgentOptions, SystemPrompt};
use sdk_claude_rust::message::{ContentBlock, Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let system_prompt = r#"You are a specialist in n8n automation.

Help me design and debug n8n workflows: triggers, nodes, credentials, expressions, data mapping, error handling, retries, and best practices.

When useful, include concrete node configuration guidance (field names and example values) and n8n expressions. Ask clarifying questions when requirements are ambiguous."#;

    let options = ClaudeAgentOptions {
        system_prompt: Some(SystemPrompt::Text(system_prompt.to_string())),
        ..Default::default()
    };

    let mut client = ClaudeSdkClient::new(Some(options), None);
    client.connect(None).await?;

    let session_id = "n8n-chat";
    println!("n8n chat (type /exit to quit)");
    print!("> ");
    std::io::stdout().flush()?;
    let mut input = String::new();
    loop {
        input.clear();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }
        let line = input.trim();
        if line.is_empty() {
            print!("> ");
            std::io::stdout().flush()?;
            continue;
        }

        if matches!(line, "/exit" | "/quit") {
            break;
        }

        client.query(line, session_id).await?;

        let stream = client.receive_response()?;
        let mut stream = Box::pin(stream);
        while let Some(message) = stream.as_mut().next().await {
            match message? {
                Message::Assistant(assistant) => {
                    for block in assistant.content {
                        match block {
                            ContentBlock::Text(text) => println!("{}", text.text),
                            ContentBlock::Thinking(_)
                            | ContentBlock::ToolUse(_)
                            | ContentBlock::ToolResult(_) => {}
                        }
                    }
                }
                Message::Result(result) => {
                    if result.is_error {
                        eprintln!("Error: {}", result.subtype);
                    }
                }
                _ => {}
            }
        }

        print!("> ");
        std::io::stdout().flush()?;
    }

    client.disconnect().await?;
    Ok(())
}

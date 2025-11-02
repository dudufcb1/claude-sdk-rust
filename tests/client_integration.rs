mod common;

use std::sync::Arc;

use futures::{stream, StreamExt};
use serde_json::json;

use sdk_claude_rust::client::{ClaudeSdkClient, ClientPrompt};
use sdk_claude_rust::config::ClaudeAgentOptions;
use sdk_claude_rust::internal::client::PromptInput;
use sdk_claude_rust::message::{ContentBlock, Message};
use sdk_claude_rust::permission::{PermissionResult, ToolPermissionContext};

use common::MockTransport;

fn assistant_message(text: &str) -> serde_json::Value {
    json!({
        "type": "assistant",
        "message": {
            "model": "claude-opus-test",
            "content": [
                {"type": "text", "text": text}
            ]
        }
    })
}

fn result_message() -> serde_json::Value {
    json!({
        "type": "result",
        "subtype": "success",
        "duration_ms": 12,
        "duration_api_ms": 10,
        "is_error": false,
        "num_turns": 1,
        "session_id": "sess-abc"
    })
}

#[tokio::test]
async fn client_receives_messages() {
    let transport = MockTransport::with_reads(vec![
        Ok(Some(assistant_message("hello"))),
        Ok(Some(result_message())),
        Ok(None),
    ]);

    let transport_arc: Arc<dyn sdk_claude_rust::transport::Transport> = transport.clone();

    let mut client = ClaudeSdkClient::new(None, Some(transport_arc));
    client
        .connect(Some(PromptInput::from("Hello")))
        .await
        .expect("connect should succeed");

    let messages = client
        .receive_response()
        .expect("stream should be available")
        .collect::<Vec<_>>()
        .await;

    assert_eq!(messages.len(), 2);
    match &messages[0] {
        Ok(Message::Assistant(msg)) => match &msg.content[0] {
            ContentBlock::Text(block) => assert_eq!(block.text, "hello"),
            other => panic!("expected text block, got {other:?}"),
        },
        other => panic!("expected assistant message, got {other:?}"),
    }

    client
        .disconnect()
        .await
        .expect("disconnect should succeed");
    assert_eq!(transport.close_calls().await, 1);
}

#[tokio::test]
async fn client_query_inserts_session_id_when_missing() {
    let transport = MockTransport::with_reads(vec![Ok(None)]);
    let transport_arc: Arc<dyn sdk_claude_rust::transport::Transport> = transport.clone();

    let mut client = ClaudeSdkClient::new(None, Some(transport_arc));
    client
        .connect(Some(PromptInput::from("Initial")))
        .await
        .expect("connect should succeed");

    let stream = stream::iter(vec![json!({
        "type": "user",
        "message": {"content": "ping"},
    })]);
    client
        .query(ClientPrompt::from_stream(stream), "session-42")
        .await
        .expect("query should write payloads");

    let writes = transport.writes().await;
    assert!(!writes.is_empty());
    let user_payload = writes
        .iter()
        .rev()
        .find(|payload| payload.get("type").and_then(|v| v.as_str()) != Some("control_request"))
        .expect("user payload should be present");
    let session_id = user_payload
        .get("session_id")
        .and_then(|val| val.as_str())
        .expect("session id should be injected");
    assert_eq!(session_id, "session-42");

    client
        .disconnect()
        .await
        .expect("disconnect should succeed");
}

#[tokio::test]
async fn client_rejects_permission_callback_without_streaming_prompt() {
    let callback = Arc::new(
        |_tool: &str,
         _input: serde_json::Map<String, serde_json::Value>,
         _ctx: ToolPermissionContext| {
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

    let transport = MockTransport::with_reads(vec![Ok(None)]);
    let transport_arc: Arc<dyn sdk_claude_rust::transport::Transport> = transport.clone();

    let mut client = ClaudeSdkClient::new(Some(options), Some(transport_arc));
    let err = client
        .connect(Some(PromptInput::from("Non streaming")))
        .await
        .expect_err("connect should fail due to invalid configuration");

    let message = err.to_string();
    assert!(message.contains("can_use_tool callback requires streaming mode"));
}

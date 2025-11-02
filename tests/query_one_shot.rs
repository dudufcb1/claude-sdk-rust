mod common;

use std::sync::Arc;

use futures::StreamExt;
use serde_json::json;

use sdk_claude_rust::message::{ContentBlock, Message};
use sdk_claude_rust::query::query;

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
        "duration_ms": 10,
        "duration_api_ms": 8,
        "is_error": false,
        "num_turns": 1,
        "session_id": "sess-123"
    })
}

#[tokio::test]
async fn query_streams_single_prompt() {
    let transport = MockTransport::with_reads(vec![
        Ok(Some(assistant_message("4"))),
        Ok(Some(result_message())),
        Ok(None),
    ]);

    let transport_arc: Arc<dyn sdk_claude_rust::transport::Transport> = transport.clone();
    let stream = query("What is 2+2?", None, Some(transport_arc))
        .await
        .expect("query should start");

    let messages = stream.collect::<Vec<_>>().await;
    assert_eq!(messages.len(), 2);

    match &messages[0] {
        Ok(Message::Assistant(msg)) => {
            assert_eq!(msg.model, "claude-opus-test");
            match &msg.content[0] {
                ContentBlock::Text(block) => assert_eq!(block.text, "4"),
                other => panic!("expected text block, got {other:?}"),
            }
        }
        other => panic!("expected assistant message, got {other:?}"),
    }

    match &messages[1] {
        Ok(Message::Result(result)) => {
            assert_eq!(result.subtype, "success");
            assert_eq!(result.session_id, "sess-123");
        }
        other => panic!("expected result message, got {other:?}"),
    }

    assert_eq!(transport.connect_calls().await, 1);
    assert!(transport.writes().await.is_empty());
}

#[tokio::test]
async fn query_sets_entrypoint_env() {
    std::env::remove_var("CLAUDE_CODE_ENTRYPOINT");

    let transport = MockTransport::with_reads(vec![Ok(None)]);
    let transport_arc: Arc<dyn sdk_claude_rust::transport::Transport> = transport.clone();

    let stream = query("ping", None, Some(transport_arc))
        .await
        .expect("query should succeed");

    let _messages = stream.collect::<Vec<_>>().await;

    let entrypoint = std::env::var("CLAUDE_CODE_ENTRYPOINT").expect("env var should be set");
    assert_eq!(entrypoint, "sdk-rs");
}

#[tokio::test]
async fn query_propagates_streaming_payloads() {
    use futures::stream;

    let transport = MockTransport::with_reads(vec![Ok(None)]);
    let transport_arc: Arc<dyn sdk_claude_rust::transport::Transport> = transport.clone();

    let prompt_stream = stream::iter(vec![json!({
        "type": "user",
        "message": { "content": "Hello" },
        "session_id": "provided",
    })]);

    let stream = query(
        sdk_claude_rust::internal::client::PromptInput::from_stream(prompt_stream),
        None,
        Some(transport_arc),
    )
    .await
    .expect("stream query should succeed");

    let _messages = stream.collect::<Vec<_>>().await;

    // prompt input should have been forwarded without injecting session id again
    let writes = transport.writes().await;
    let non_control = writes
        .iter()
        .filter(|payload| payload.get("type").and_then(|v| v.as_str()) != Some("control_request"))
        .count();
    assert_eq!(
        non_control, 0,
        "no additional user payloads should be written"
    );
}

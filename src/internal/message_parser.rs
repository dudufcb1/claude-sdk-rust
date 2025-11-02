//! Parse raw CLI JSON messages into strongly typed structures.

use serde_json::Value;

use crate::error::{MessageParseError, SdkError};
use crate::message::{
    AssistantMessage, ContentBlock, Message, ResultMessage, StreamEvent, SystemMessage,
    ToolResultBlock, ToolUseBlock, UserMessage, UserMessageContent,
};

/// Convert a serde_json::Value into a strongly typed `Message` value.
pub fn parse_message(raw: &Value) -> Result<Message, SdkError> {
    let object = raw.as_object().ok_or_else(|| {
        MessageParseError::new(
            format!(
                "Invalid message data type (expected object, got {})",
                value_type_name(raw)
            ),
            Some(raw.clone()),
        )
    })?;

    let message_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| MessageParseError::new("Message missing 'type' field", Some(raw.clone())))?;

    match message_type {
        "user" => parse_user_message(raw),
        "assistant" => parse_assistant_message(raw),
        "system" => parse_system_message(raw),
        "result" => parse_result_message(raw),
        "stream_event" => parse_stream_event(raw),
        other => Err(MessageParseError::new(
            format!("Unknown message type: {other}"),
            Some(raw.clone()),
        )
        .into()),
    }
}

fn parse_user_message(raw: &Value) -> Result<Message, SdkError> {
    let message_object = raw.get("message").and_then(Value::as_object);

    let content_value = message_object
        .and_then(|message| message.get("content"))
        .or_else(|| raw.get("content"))
        .ok_or_else(|| MessageParseError::new("User message missing content", Some(raw.clone())))?;

    let content = if content_value.is_string() {
        UserMessageContent::Text(
            content_value
                .as_str()
                .ok_or_else(|| MessageParseError::new("Invalid string content", Some(raw.clone())))?
                .to_string(),
        )
    } else if content_value.is_array() {
        let blocks = content_value
            .as_array()
            .ok_or_else(|| MessageParseError::new("Invalid content array", Some(raw.clone())))?
            .iter()
            .map(parse_content_block)
            .collect::<Result<Vec<_>, _>>()?;
        UserMessageContent::Blocks(blocks)
    } else {
        return Err(
            MessageParseError::new("Unrecognized user message content", Some(raw.clone())).into(),
        );
    };

    let parent_tool_use_id = raw
        .get("parent_tool_use_id")
        .or_else(|| raw.get("parentToolUseId"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    Ok(Message::User(UserMessage {
        content,
        parent_tool_use_id,
    }))
}

fn parse_assistant_message(raw: &Value) -> Result<Message, SdkError> {
    let message_object = raw.get("message").and_then(Value::as_object);

    let content_value = message_object
        .and_then(|message| message.get("content"))
        .or_else(|| raw.get("content"))
        .ok_or_else(|| {
            MessageParseError::new("Assistant message missing content", Some(raw.clone()))
        })?;

    let content = content_value
        .as_array()
        .ok_or_else(|| MessageParseError::new("Invalid assistant content", Some(raw.clone())))?
        .iter()
        .map(parse_content_block)
        .collect::<Result<Vec<_>, _>>()?;

    let model_value = message_object
        .and_then(|message| message.get("model"))
        .or_else(|| raw.get("model"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MessageParseError::new("Assistant message missing model", Some(raw.clone()))
        })?
        .to_string();

    let parent_tool_use_id = raw
        .get("parent_tool_use_id")
        .or_else(|| raw.get("parentToolUseId"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    Ok(Message::Assistant(AssistantMessage {
        content,
        model: model_value,
        parent_tool_use_id,
    }))
}

fn parse_system_message(raw: &Value) -> Result<Message, SdkError> {
    let subtype = raw
        .get("subtype")
        .and_then(Value::as_str)
        .ok_or_else(|| MessageParseError::new("System message missing subtype", Some(raw.clone())))?
        .to_string();

    let data = raw
        .as_object()
        .ok_or_else(|| MessageParseError::new("System message missing data", Some(raw.clone())))?
        .clone();

    Ok(Message::System(SystemMessage { subtype, data }))
}

fn parse_result_message(raw: &Value) -> Result<Message, SdkError> {
    let subtype = raw
        .get("subtype")
        .and_then(Value::as_str)
        .ok_or_else(|| MessageParseError::new("Result message missing subtype", Some(raw.clone())))?
        .to_string();

    let duration_ms = get_i64(raw, "duration_ms")?;
    let duration_api_ms = get_i64(raw, "duration_api_ms")?;
    let is_error = get_bool(raw, "is_error")?;
    let num_turns = get_i64(raw, "num_turns")?;
    let session_id = raw
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MessageParseError::new("Result message missing session_id", Some(raw.clone()))
        })?
        .to_string();

    let total_cost_usd = raw.get("total_cost_usd").and_then(Value::as_f64);
    let usage = raw.get("usage").and_then(Value::as_object).cloned();
    let result = raw
        .get("result")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    Ok(Message::Result(ResultMessage {
        subtype,
        duration_ms,
        duration_api_ms,
        is_error,
        num_turns,
        session_id,
        total_cost_usd,
        usage,
        result,
    }))
}

fn parse_stream_event(raw: &Value) -> Result<Message, SdkError> {
    let uuid = raw
        .get("uuid")
        .and_then(Value::as_str)
        .ok_or_else(|| MessageParseError::new("Stream event missing uuid", Some(raw.clone())))?
        .to_string();
    let session_id = raw
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            MessageParseError::new("Stream event missing session_id", Some(raw.clone()))
        })?
        .to_string();
    let event = raw
        .get("event")
        .ok_or_else(|| {
            MessageParseError::new("Stream event missing event payload", Some(raw.clone()))
        })?
        .clone();

    let parent_tool_use_id = raw
        .get("parent_tool_use_id")
        .or_else(|| raw.get("parentToolUseId"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    Ok(Message::StreamEvent(StreamEvent {
        uuid,
        session_id,
        event,
        parent_tool_use_id,
    }))
}

fn parse_content_block(raw: &Value) -> Result<ContentBlock, SdkError> {
    let kind = raw
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| MessageParseError::new("Content block missing type", Some(raw.clone())))?;

    match kind {
        "text" => {
            let text = raw
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    MessageParseError::new("Text block missing text", Some(raw.clone()))
                })?
                .to_string();
            Ok(ContentBlock::Text(crate::message::TextBlock { text }))
        }
        "thinking" => {
            let thinking = raw
                .get("thinking")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    MessageParseError::new("Thinking block missing text", Some(raw.clone()))
                })?
                .to_string();
            let signature = raw
                .get("signature")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    MessageParseError::new("Thinking block missing signature", Some(raw.clone()))
                })?
                .to_string();
            Ok(ContentBlock::Thinking(crate::message::ThinkingBlock {
                thinking,
                signature,
            }))
        }
        "tool_use" => {
            let id = raw
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    MessageParseError::new("Tool use block missing id", Some(raw.clone()))
                })?
                .to_string();
            let name = raw
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    MessageParseError::new("Tool use block missing name", Some(raw.clone()))
                })?
                .to_string();
            let input = raw
                .get("input")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    MessageParseError::new("Tool use block missing input", Some(raw.clone()))
                })?
                .clone();
            Ok(ContentBlock::ToolUse(ToolUseBlock { id, name, input }))
        }
        "tool_result" => {
            let tool_use_id = raw
                .get("tool_use_id")
                .or_else(|| raw.get("toolUseId"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    MessageParseError::new(
                        "Tool result block missing tool_use_id",
                        Some(raw.clone()),
                    )
                })?
                .to_string();
            let content = raw.get("content").cloned();
            let is_error = raw.get("is_error").and_then(Value::as_bool);
            Ok(ContentBlock::ToolResult(ToolResultBlock {
                tool_use_id,
                content,
                is_error,
            }))
        }
        other => Err(MessageParseError::new(
            format!("Unknown content block type: {other}"),
            Some(raw.clone()),
        )
        .into()),
    }
}

fn get_i64(raw: &Value, key: &str) -> Result<i64, SdkError> {
    raw.get(key).and_then(Value::as_i64).ok_or_else(|| {
        MessageParseError::new(format!("Missing integer field: {key}"), Some(raw.clone())).into()
    })
}

fn get_bool(raw: &Value, key: &str) -> Result<bool, SdkError> {
    raw.get(key).and_then(Value::as_bool).ok_or_else(|| {
        MessageParseError::new(format!("Missing boolean field: {key}"), Some(raw.clone())).into()
    })
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_user_text_message() {
        let raw = json!({
            "type": "user",
            "message": {
                "content": [
                    {"type": "text", "text": "Hello"}
                ]
            }
        });

        let message = parse_message(&raw).expect("failed to parse user message");
        match message {
            Message::User(user) => match user.content {
                UserMessageContent::Blocks(blocks) => {
                    assert_eq!(blocks.len(), 1);
                    match &blocks[0] {
                        ContentBlock::Text(block) => assert_eq!(block.text, "Hello"),
                        other => panic!("expected text block, got {other:?}"),
                    }
                }
                other => panic!("expected block content, got {other:?}"),
            },
            other => panic!("expected user message, got {other:?}"),
        }
    }

    #[test]
    fn parses_user_message_with_tool_use_and_result() {
        let raw = json!({
            "type": "user",
            "parent_tool_use_id": "tool_parent",
            "message": {
                "content": [
                    {"type": "tool_use", "id": "tool_123", "name": "Read", "input": {"path": "test.txt"}},
                    {"type": "tool_result", "tool_use_id": "tool_123", "content": "done"}
                ]
            }
        });

        let message = parse_message(&raw).expect("failed to parse user message with tool blocks");
        match message {
            Message::User(user) => {
                assert_eq!(user.parent_tool_use_id.as_deref(), Some("tool_parent"));
                let blocks = match user.content {
                    UserMessageContent::Blocks(blocks) => blocks,
                    other => panic!("expected blocks content, got {other:?}"),
                };
                assert_eq!(blocks.len(), 2);
                match &blocks[0] {
                    ContentBlock::ToolUse(tool) => {
                        assert_eq!(tool.id, "tool_123");
                        assert_eq!(tool.name, "Read");
                        assert_eq!(
                            tool.input.get("path").and_then(Value::as_str),
                            Some("test.txt")
                        );
                    }
                    other => panic!("expected tool_use, got {other:?}"),
                }
                match &blocks[1] {
                    ContentBlock::ToolResult(result) => {
                        assert_eq!(result.tool_use_id, "tool_123");
                        assert_eq!(
                            result.content.as_ref().and_then(Value::as_str),
                            Some("done")
                        );
                        assert!(result.is_error.is_none());
                    }
                    other => panic!("expected tool_result, got {other:?}"),
                }
            }
            other => panic!("expected user message, got {other:?}"),
        }
    }

    #[test]
    fn parses_assistant_message_with_thinking_block() {
        let raw = json!({
            "type": "assistant",
            "message": {
                "model": "claude-opus",
                "content": [
                    {"type": "thinking", "thinking": "calculating", "signature": "sig"},
                    {"type": "text", "text": "answer"}
                ]
            }
        });

        let message = parse_message(&raw).expect("failed to parse assistant message");
        match message {
            Message::Assistant(assistant) => {
                assert_eq!(assistant.model, "claude-opus");
                assert_eq!(assistant.content.len(), 2);
                match &assistant.content[0] {
                    ContentBlock::Thinking(block) => {
                        assert_eq!(block.thinking, "calculating");
                        assert_eq!(block.signature, "sig");
                    }
                    other => panic!("expected thinking block, got {other:?}"),
                }
                assert!(matches!(assistant.content[1], ContentBlock::Text(_)));
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
    }

    #[test]
    fn parses_system_message() {
        let raw = json!({
            "type": "system",
            "subtype": "start",
            "note": "init"
        });

        let message = parse_message(&raw).expect("failed to parse system message");
        match message {
            Message::System(system) => {
                assert_eq!(system.subtype, "start");
                assert_eq!(
                    system.data.get("note").and_then(Value::as_str),
                    Some("init")
                );
            }
            other => panic!("expected system message, got {other:?}"),
        }
    }

    #[test]
    fn parses_result_message() {
        let raw = json!({
            "type": "result",
            "subtype": "success",
            "duration_ms": 1000,
            "duration_api_ms": 900,
            "is_error": false,
            "num_turns": 2,
            "session_id": "sess",
            "result": "ok"
        });

        let message = parse_message(&raw).expect("failed to parse result message");
        match message {
            Message::Result(result) => {
                assert_eq!(result.subtype, "success");
                assert_eq!(result.duration_ms, 1000);
                assert_eq!(result.result.as_deref(), Some("ok"));
            }
            other => panic!("expected result message, got {other:?}"),
        }
    }

    #[test]
    fn parses_stream_event() {
        let raw = json!({
            "type": "stream_event",
            "uuid": "event-1",
            "session_id": "sess",
            "event": {"delta": "..."}
        });

        let message = parse_message(&raw).expect("failed to parse stream event");
        match message {
            Message::StreamEvent(event) => {
                assert_eq!(event.uuid, "event-1");
                assert_eq!(event.session_id, "sess");
                assert!(event.event.get("delta").is_some());
            }
            other => panic!("expected stream event, got {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_message_data_type() {
        let raw = serde_json::Value::String("oops".into());
        let err = parse_message(&raw).expect_err("expected parse error");
        match err {
            SdkError::MessageParse(parse_err) => {
                assert!(parse_err.message().contains("Invalid message data type"));
                assert!(parse_err.message().contains("string"));
                assert_eq!(parse_err.data(), Some(&raw));
            }
            other => panic!("expected MessageParse error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_type_field() {
        let raw = json!({"message": {"content": []}});
        let err = parse_message(&raw).expect_err("expected parse error");
        match err {
            SdkError::MessageParse(parse_err) => {
                assert!(parse_err.message().contains("Message missing 'type' field"));
            }
            other => panic!("expected MessageParse error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_message_type() {
        let raw = json!({"type": "unknown"});
        let err = parse_message(&raw).expect_err("expected parse error");
        match err {
            SdkError::MessageParse(parse_err) => {
                assert!(parse_err.message().contains("Unknown message type"));
                assert_eq!(parse_err.data(), Some(&raw));
            }
            other => panic!("expected MessageParse error, got {other:?}"),
        }
    }
}

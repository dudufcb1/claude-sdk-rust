use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use sdk_claude_rust::error::SdkError;
use sdk_claude_rust::transport::Transport;

#[derive(Default)]
struct MockTransportState {
    reads: VecDeque<Result<Option<Value>, SdkError>>,
    writes: Vec<Value>,
    connect_calls: usize,
    end_input_calls: usize,
    close_calls: usize,
}

/// Asynchronous transport stub used by integration tests to mirror the Python test harness.
#[derive(Default)]
pub struct MockTransport {
    state: Mutex<MockTransportState>,
    ready: AtomicBool,
}

#[allow(dead_code)]
impl MockTransport {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(MockTransportState {
                reads: VecDeque::new(),
                writes: Vec::new(),
                connect_calls: 0,
                end_input_calls: 0,
                close_calls: 0,
            }),
            ready: AtomicBool::new(true),
        })
    }

    pub fn with_reads<T>(reads: T) -> Arc<Self>
    where
        T: IntoIterator<Item = Result<Option<Value>, SdkError>>,
    {
        let state = MockTransportState {
            reads: reads.into_iter().collect(),
            ..Default::default()
        };
        Arc::new(Self {
            state: Mutex::new(state),
            ready: AtomicBool::new(true),
        })
    }

    pub async fn enqueue_read(&self, value: Result<Option<Value>, SdkError>) {
        let mut state = self.state.lock().await;
        state.reads.push_back(value);
    }

    pub async fn writes(&self) -> Vec<Value> {
        let state = self.state.lock().await;
        state.writes.clone()
    }

    pub async fn connect_calls(&self) -> usize {
        let state = self.state.lock().await;
        state.connect_calls
    }

    pub async fn end_input_calls(&self) -> usize {
        let state = self.state.lock().await;
        state.end_input_calls
    }

    pub async fn close_calls(&self) -> usize {
        let state = self.state.lock().await;
        state.close_calls
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn connect(&self) -> Result<(), SdkError> {
        let mut state = self.state.lock().await;
        state.connect_calls += 1;
        Ok(())
    }

    async fn write(&self, payload: &Value) -> Result<(), SdkError> {
        let mut state = self.state.lock().await;
        state.writes.push(payload.clone());

        if payload
            .get("type")
            .and_then(Value::as_str)
            .map(|value| value == "control_request")
            .unwrap_or(false)
        {
            if let Some(request_id) = payload.get("request_id").and_then(Value::as_str) {
                let response = json!({
                    "type": "control_response",
                    "response": {
                        "subtype": "success",
                        "request_id": request_id,
                        "response": serde_json::Value::Null,
                    }
                });
                state.reads.push_front(Ok(Some(response)));
            }
        }

        Ok(())
    }

    async fn read(&self) -> Result<Option<Value>, SdkError> {
        let mut state = self.state.lock().await;
        if let Some(next) = state.reads.pop_front() {
            next
        } else {
            Ok(None)
        }
    }

    async fn end_input(&self) -> Result<(), SdkError> {
        let mut state = self.state.lock().await;
        state.end_input_calls += 1;
        Ok(())
    }

    async fn close(&self) -> Result<(), SdkError> {
        let mut state = self.state.lock().await;
        state.close_calls += 1;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

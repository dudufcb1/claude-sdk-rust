//! Core control protocol handling for the SDK.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::{Stream, StreamExt};
use serde_json::{json, Map, Value};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::error::SdkError;
use crate::hooks::{HookCallback, HookContext, HookEvent, HookInput, HookMatcher};
use crate::internal::message_parser;
use crate::mcp::{McpToolCallResult, McpToolContent, McpToolInfo, SdkMcpServer};
use crate::message::Message;
use crate::permission::{
    CanUseToolCallback, PermissionMode, PermissionResult, PermissionUpdate, ToolPermissionContext,
};
use crate::transport::Transport;

const CONTROL_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MESSAGE_CHANNEL_CAPACITY: usize = 100;

type ControlResponder = oneshot::Sender<Result<Value, SdkError>>;
type HookCallbackHandle = Arc<dyn HookCallback>;
type ToolPermissionCallbackHandle = Arc<dyn CanUseToolCallback>;
type McpServerHandle = Arc<dyn SdkMcpServer>;

/// Query orchestrates the communication with the Claude CLI transport.
pub struct Query<T: Transport + ?Sized> {
    inner: Arc<QueryInner<T>>,
}

impl<T: Transport + ?Sized> Clone for Query<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct QueryInner<T: Transport + ?Sized> {
    transport: Arc<T>,
    is_streaming_mode: bool,
    can_use_tool: Option<ToolPermissionCallbackHandle>,
    hooks: Mutex<Option<HashMap<HookEvent, Vec<HookMatcher>>>>,
    sdk_mcp_servers: HashMap<String, McpServerHandle>,
    pending_control: Mutex<HashMap<String, ControlResponder>>,
    hook_callbacks: Mutex<HashMap<String, HookCallbackHandle>>,
    message_tx: Mutex<Option<mpsc::Sender<Result<Message, SdkError>>>>,
    message_rx: Mutex<mpsc::Receiver<Result<Message, SdkError>>>,
    read_handle: Mutex<Option<JoinHandle<()>>>,
    next_callback_id: AtomicU64,
    request_counter: AtomicU64,
    initialized: AtomicBool,
    initialization_result: Mutex<Option<Value>>,
    closed: AtomicBool,
}

impl<T> Query<T>
where
    T: Transport + ?Sized + 'static,
{
    /// Create a new query wrapper around the provided transport and callbacks.
    pub fn new(
        transport: Arc<T>,
        is_streaming_mode: bool,
        can_use_tool: Option<ToolPermissionCallbackHandle>,
        hooks: Option<HashMap<HookEvent, Vec<HookMatcher>>>,
        sdk_mcp_servers: HashMap<String, McpServerHandle>,
    ) -> Self {
        let (message_tx, message_rx) = mpsc::channel(MESSAGE_CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(QueryInner {
                transport,
                is_streaming_mode,
                can_use_tool,
                hooks: Mutex::new(hooks),
                sdk_mcp_servers,
                pending_control: Mutex::new(HashMap::new()),
                hook_callbacks: Mutex::new(HashMap::new()),
                message_tx: Mutex::new(Some(message_tx)),
                message_rx: Mutex::new(message_rx),
                read_handle: Mutex::new(None),
                next_callback_id: AtomicU64::new(0),
                request_counter: AtomicU64::new(0),
                initialized: AtomicBool::new(false),
                initialization_result: Mutex::new(None),
                closed: AtomicBool::new(false),
            }),
        }
    }

    /// Returns whether the query is operating in streaming mode.
    pub fn is_streaming_mode(&self) -> bool {
        self.inner.is_streaming_mode
    }

    /// Returns whether the query has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    /// Start the background reader if it has not already been started.
    pub async fn start(&self) -> Result<(), SdkError> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(SdkError::Message("query is closed".into()));
        }

        let mut handle_guard = self.inner.read_handle.lock().await;
        if handle_guard.is_some() {
            return Ok(());
        }

        let inner = Arc::clone(&self.inner);
        let handle = tokio::spawn(async move {
            Query { inner }.read_loop().await;
        });
        *handle_guard = Some(handle);
        Ok(())
    }

    /// Initialize the control protocol and register hooks when in streaming mode.
    pub async fn initialize(&self) -> Result<Option<Value>, SdkError> {
        if !self.inner.is_streaming_mode {
            return Ok(None);
        }

        self.start().await?;
        let hooks_config = self.prepare_hooks_configuration().await?;

        let mut request = Map::new();
        request.insert("subtype".into(), Value::String("initialize".into()));
        if let Some(config) = hooks_config {
            request.insert("hooks".into(), config);
        } else {
            request.insert("hooks".into(), Value::Null);
        }

        let response = self.send_control_request(Value::Object(request)).await?;
        self.inner.initialized.store(true, Ordering::SeqCst);
        {
            let mut guard = self.inner.initialization_result.lock().await;
            *guard = Some(response.clone());
        }
        Ok(Some(response))
    }

    /// Stream input messages to the transport.
    pub async fn stream_input<S>(&self, mut input: S) -> Result<(), SdkError>
    where
        S: Stream<Item = Value> + Unpin + Send,
    {
        log::debug!("[stream_input] Starting stream consumption");
        let mut wrote_any = false;
        while let Some(message) = input.next().await {
            if self.inner.closed.load(Ordering::SeqCst) {
                log::debug!("[stream_input] Query closed, breaking");
                break;
            }
            log::debug!("[stream_input] Writing message to transport");
            self.inner.transport.write(&message).await?;
            wrote_any = true;
        }
        if wrote_any {
            log::debug!("[stream_input] Wrote messages, calling end_input");
            self.inner.transport.end_input().await?;
        } else {
            log::debug!("[stream_input] No messages written, keeping stdin open");
        }
        log::debug!("[stream_input] Stream input completed");
        Ok(())
    }

    /// Retrieve the next SDK message, if available.
    pub async fn next_message(&self) -> Result<Option<Message>, SdkError> {
        let mut receiver = self.inner.message_rx.lock().await;
        match receiver.recv().await {
            Some(Ok(message)) => Ok(Some(message)),
            Some(Err(err)) => Err(err),
            None => Ok(None),
        }
    }

    /// Interrupt the current run via the control protocol.
    pub async fn interrupt(&self) -> Result<(), SdkError> {
        self.send_control_request(json!({ "subtype": "interrupt" }))
            .await
            .map(|_| ())
    }

    /// Update the permission mode via the control protocol.
    pub async fn set_permission_mode(&self, mode: PermissionMode) -> Result<(), SdkError> {
        self.send_control_request(json!({
            "subtype": "set_permission_mode",
            "mode": mode.as_str(),
        }))
        .await
        .map(|_| ())
    }

    /// Update the active model via the control protocol.
    pub async fn set_model(&self, model: Option<String>) -> Result<(), SdkError> {
        let mut request = Map::new();
        request.insert("subtype".into(), Value::String("set_model".into()));
        request.insert(
            "model".into(),
            model.map(Value::String).unwrap_or(Value::Null),
        );
        self.send_control_request(Value::Object(request))
            .await
            .map(|_| ())
    }

    /// Close the query and underlying transport, cancelling any pending work.
    pub async fn close(&self) -> Result<(), SdkError> {
        if self.inner.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        if let Some(handle) = self.inner.read_handle.lock().await.take() {
            handle.abort();
            let _ = handle.await;
        }

        {
            let mut pending = self.inner.pending_control.lock().await;
            for (_, responder) in pending.drain() {
                let _ = responder.send(Err(SdkError::Message("query closed".into())));
            }
        }

        {
            let mut tx_guard = self.inner.message_tx.lock().await;
            tx_guard.take();
        }

        self.inner.transport.close().await
    }

    /// Previously returned initialization payload, if initialization has completed.
    pub async fn initialization_result(&self) -> Option<Value> {
        self.inner.initialization_result.lock().await.clone()
    }

    async fn read_loop(self) {
        loop {
            if self.inner.closed.load(Ordering::SeqCst) {
                break;
            }

            match self.inner.transport.read().await {
                Ok(Some(raw)) => {
                    if let Err(err) = self.route_incoming_message(raw).await {
                        let _ = self.enqueue_message(Err(err)).await;
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    let _ = self.enqueue_message(Err(err)).await;
                    break;
                }
            }
        }

        {
            let mut tx_guard = self.inner.message_tx.lock().await;
            tx_guard.take();
        }
    }

    async fn route_incoming_message(&self, raw: Value) -> Result<(), SdkError> {
        let message_type = raw.get("type").and_then(Value::as_str);
        match message_type {
            Some("control_response") => self.handle_control_response(raw).await,
            Some("control_request") => {
                self.spawn_control_request(raw);
                Ok(())
            }
            Some("control_cancel_request") => Ok(()),
            _ => {
                let parsed = message_parser::parse_message(&raw);
                self.enqueue_message(parsed).await
            }
        }
    }

    fn spawn_control_request(&self, request: Value) {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            Query { inner }.process_control_request(request).await;
        });
    }

    async fn enqueue_message(&self, payload: Result<Message, SdkError>) -> Result<(), SdkError> {
        let sender = {
            let guard = self.inner.message_tx.lock().await;
            guard.as_ref().cloned()
        };

        if let Some(sender) = sender {
            sender
                .send(payload)
                .await
                .map_err(|err| SdkError::Message(format!("failed to enqueue message: {err}")))
        } else {
            Ok(())
        }
    }

    async fn handle_control_response(&self, message: Value) -> Result<(), SdkError> {
        let response = message
            .get("response")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| SdkError::Message("control response missing 'response' field".into()))?;

        let request_id = response
            .get("request_id")
            .and_then(Value::as_str)
            .ok_or_else(|| SdkError::Message("control response missing request_id".into()))?
            .to_string();

        let subtype = response
            .get("subtype")
            .and_then(Value::as_str)
            .ok_or_else(|| SdkError::Message("control response missing subtype".into()))?;

        let responder = {
            let mut guard = self.inner.pending_control.lock().await;
            guard.remove(&request_id)
        };

        if let Some(responder) = responder {
            match subtype {
                "error" => {
                    let message = response
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("Unknown error")
                        .to_string();
                    let _ = responder.send(Err(SdkError::Message(message)));
                }
                _ => {
                    let payload = response.get("response").cloned().unwrap_or(Value::Null);
                    let _ = responder.send(Ok(payload));
                }
            }
        }

        Ok(())
    }

    async fn process_control_request(&self, request: Value) {
        let request_id = match request.get("request_id").and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => return,
        };

        let payload = match request.get("request").and_then(Value::as_object).cloned() {
            Some(payload) => payload,
            None => {
                let _ = self
                    .send_error_response(
                        &request_id,
                        "Control request missing 'request' field".into(),
                    )
                    .await;
                return;
            }
        };

        match self.dispatch_control_request(&payload).await {
            Ok(response) => {
                let _ = self.send_success_response(&request_id, response).await;
            }
            Err(err) => {
                let _ = self.send_error_response(&request_id, err.to_string()).await;
            }
        }
    }

    async fn dispatch_control_request(
        &self,
        payload: &Map<String, Value>,
    ) -> Result<Value, SdkError> {
        let subtype = payload
            .get("subtype")
            .and_then(Value::as_str)
            .ok_or_else(|| SdkError::Message("control request missing subtype".into()))?;

        match subtype {
            "can_use_tool" => self.handle_permission_request(payload).await,
            "hook_callback" => self.handle_hook_callback(payload).await,
            "mcp_message" => self.handle_mcp_message(payload).await,
            other => Err(SdkError::Message(format!(
                "unsupported control request subtype: {other}",
            ))),
        }
    }

    async fn handle_permission_request(
        &self,
        payload: &Map<String, Value>,
    ) -> Result<Value, SdkError> {
        let callback = self
            .inner
            .can_use_tool
            .as_ref()
            .ok_or_else(|| SdkError::Message("canUseTool callback is not provided".into()))?;

        let tool_name = payload
            .get("tool_name")
            .and_then(Value::as_str)
            .ok_or_else(|| SdkError::Message("permission request missing tool_name".into()))?;

        let input_value = payload
            .get("input")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| SdkError::Message("permission request missing input".into()))?;

        let suggestions_raw = payload
            .get("permission_suggestions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let suggestions = deserialize_permission_suggestions(&suggestions_raw);

        let context = ToolPermissionContext {
            signal: None,
            suggestions,
        };

        let result = callback.call(tool_name, input_value.clone(), context).await;

        match result {
            PermissionResult::Allow {
                updated_input,
                updated_permissions,
            } => {
                let mut response = Map::new();
                response.insert("behavior".into(), Value::String("allow".into()));
                let final_input = updated_input.unwrap_or(input_value);
                response.insert("updatedInput".into(), Value::Object(final_input));
                if let Some(updates) = updated_permissions {
                    let serialized = updates
                        .into_iter()
                        .map(|update| update.to_control_payload())
                        .collect();
                    response.insert("updatedPermissions".into(), Value::Array(serialized));
                }
                Ok(Value::Object(response))
            }
            PermissionResult::Deny { message, interrupt } => {
                let mut response = Map::new();
                response.insert("behavior".into(), Value::String("deny".into()));
                if !message.is_empty() {
                    response.insert("message".into(), Value::String(message));
                }
                if interrupt {
                    response.insert("interrupt".into(), Value::Bool(true));
                }
                Ok(Value::Object(response))
            }
        }
    }

    async fn handle_hook_callback(&self, payload: &Map<String, Value>) -> Result<Value, SdkError> {
        let callback_id = payload
            .get("callback_id")
            .and_then(Value::as_str)
            .ok_or_else(|| SdkError::Message("hook callback missing callback_id".into()))?
            .to_string();

        let callback = {
            let callbacks = self.inner.hook_callbacks.lock().await;
            callbacks.get(&callback_id).cloned()
        }
        .ok_or_else(|| {
            SdkError::Message(format!("No hook callback found for ID: {callback_id}"))
        })?;

        let input_value = payload
            .get("input")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        let hook_input: HookInput = serde_json::from_value(input_value)?;

        let tool_use_id = payload
            .get("tool_use_id")
            .and_then(Value::as_str)
            .map(|s| s.to_string());

        let output = callback
            .call(hook_input, tool_use_id, HookContext { signal: None })
            .await;

        let output_value = serde_json::to_value(output)?;
        Ok(convert_hook_output_for_cli(output_value))
    }

    async fn handle_mcp_message(&self, payload: &Map<String, Value>) -> Result<Value, SdkError> {
        let server_name = payload
            .get("server_name")
            .and_then(Value::as_str)
            .ok_or_else(|| SdkError::Message("MCP request missing server_name".into()))?;

        let message_value = payload
            .get("message")
            .cloned()
            .ok_or_else(|| SdkError::Message("MCP request missing message payload".into()))?;

        let message = message_value
            .as_object()
            .cloned()
            .ok_or_else(|| SdkError::Message("MCP message must be an object".into()))?;

        let server = self
            .inner
            .sdk_mcp_servers
            .get(server_name)
            .cloned()
            .ok_or_else(|| SdkError::Message(format!("Server '{server_name}' not found")))?;

        let method = message
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| SdkError::Message("MCP message missing method".into()))?;

        match method {
            "initialize" => Ok(build_mcp_initialize_response(&message, &server)),
            "tools/list" => self.mcp_list_tools(&message, server).await,
            "tools/call" => self.mcp_call_tool(&message, server).await,
            "notifications/initialized" => Ok(json!({ "jsonrpc": "2.0", "result": {} })),
            other => Ok(jsonrpc_error(
                message.get("id").cloned().unwrap_or(Value::Null),
                -32601,
                format!("Method '{other}' not found"),
            )),
        }
    }

    async fn mcp_list_tools(
        &self,
        message: &Map<String, Value>,
        server: McpServerHandle,
    ) -> Result<Value, SdkError> {
        let id_value = message.get("id").cloned().unwrap_or(Value::Null);
        match server.list_tools().await {
            Ok(tools) => {
                let tools_json = convert_mcp_tool_list(tools);
                let mut result = Map::new();
                result.insert("tools".into(), Value::Array(tools_json));

                let mut response = Map::new();
                response.insert("jsonrpc".into(), Value::String("2.0".into()));
                response.insert("id".into(), id_value);
                response.insert("result".into(), Value::Object(result));
                Ok(Value::Object(response))
            }
            Err(err) => Ok(jsonrpc_error(id_value, -32603, err.to_string())),
        }
    }

    async fn mcp_call_tool(
        &self,
        message: &Map<String, Value>,
        server: McpServerHandle,
    ) -> Result<Value, SdkError> {
        let id_value = message.get("id").cloned().unwrap_or(Value::Null);
        let params = message
            .get("params")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        let tool_name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| SdkError::Message("tools/call missing name parameter".into()))?;

        let arguments = params
            .get("arguments")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        match server.call_tool(tool_name, arguments).await {
            Ok(result) => {
                let payload = convert_mcp_call_result(result);
                let mut response = Map::new();
                response.insert("jsonrpc".into(), Value::String("2.0".into()));
                response.insert("id".into(), id_value);
                response.insert("result".into(), payload);
                Ok(Value::Object(response))
            }
            Err(err) => Ok(jsonrpc_error(id_value, -32603, err.to_string())),
        }
    }

    async fn send_success_response(
        &self,
        request_id: &str,
        payload: Value,
    ) -> Result<(), SdkError> {
        let envelope = json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": payload,
            }
        });
        self.inner.transport.write(&envelope).await
    }

    async fn send_error_response(&self, request_id: &str, message: String) -> Result<(), SdkError> {
        let envelope = json!({
            "type": "control_response",
            "response": {
                "subtype": "error",
                "request_id": request_id,
                "error": message,
            }
        });
        self.inner.transport.write(&envelope).await
    }

    async fn send_control_request(&self, request: Value) -> Result<Value, SdkError> {
        if !self.inner.is_streaming_mode {
            return Err(SdkError::Message(
                "control requests require streaming mode".into(),
            ));
        }

        self.start().await?;

        let counter = self.inner.request_counter.fetch_add(1, Ordering::SeqCst) + 1;
        let request_id = format!("req_{}_{}", counter, unique_request_suffix());

        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.inner.pending_control.lock().await;
            pending.insert(request_id.clone(), sender);
        }

        let envelope = json!({
            "type": "control_request",
            "request_id": request_id.clone(),
            "request": request,
        });

        if let Err(err) = self.inner.transport.write(&envelope).await {
            let mut pending = self.inner.pending_control.lock().await;
            pending.remove(&request_id);
            return Err(err);
        }

        match timeout(CONTROL_REQUEST_TIMEOUT, receiver).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => Err(SdkError::Message("control response channel closed".into())),
            Err(err) => {
                let mut pending = self.inner.pending_control.lock().await;
                pending.remove(&request_id);
                Err(SdkError::Timeout(err))
            }
        }
    }

    async fn prepare_hooks_configuration(&self) -> Result<Option<Value>, SdkError> {
        let mut hooks_guard = self.inner.hooks.lock().await;
        let hooks = hooks_guard.take();
        drop(hooks_guard);

        let Some(mut hook_map) = hooks else {
            return Ok(None);
        };

        if hook_map.is_empty() {
            return Ok(None);
        }

        let mut callbacks_guard = self.inner.hook_callbacks.lock().await;
        let mut config = Map::new();

        for (event, mut matchers) in hook_map.drain() {
            if matchers.is_empty() {
                continue;
            }

            let mut matcher_entries = Vec::new();
            for matcher in matchers.iter_mut() {
                if matcher.hooks.is_empty() {
                    continue;
                }

                let mut callback_ids = Vec::new();
                for hook in matcher.hooks.drain(..) {
                    let id = format!(
                        "hook_{}",
                        self.inner.next_callback_id.fetch_add(1, Ordering::SeqCst)
                    );
                    callbacks_guard.insert(id.clone(), hook);
                    callback_ids.push(Value::String(id));
                }

                if callback_ids.is_empty() {
                    continue;
                }

                let mut entry = Map::new();
                if let Some(matcher_value) = matcher.matcher.clone() {
                    entry.insert("matcher".into(), matcher_value);
                }
                entry.insert("hookCallbackIds".into(), Value::Array(callback_ids));
                matcher_entries.push(Value::Object(entry));
            }

            if !matcher_entries.is_empty() {
                config.insert(event.as_str().to_string(), Value::Array(matcher_entries));
            }
        }

        drop(callbacks_guard);

        if config.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Value::Object(config)))
        }
    }
}

fn unique_request_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let pid = std::process::id() as u128;
    format!("{:x}", timestamp ^ pid)
}

fn convert_hook_output_for_cli(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut converted = Map::new();
            for (key, val) in map.into_iter() {
                let converted_value = convert_hook_output_for_cli(val);
                match key.as_str() {
                    "async_" => {
                        converted.insert("async".into(), converted_value);
                    }
                    "continue_" => {
                        converted.insert("continue".into(), converted_value);
                    }
                    _ => {
                        converted.insert(key, converted_value);
                    }
                }
            }
            Value::Object(converted)
        }
        Value::Array(items) => {
            Value::Array(items.into_iter().map(convert_hook_output_for_cli).collect())
        }
        other => other,
    }
}

fn deserialize_permission_suggestions(entries: &[Value]) -> Vec<PermissionUpdate> {
    entries
        .iter()
        .filter_map(|entry| serde_json::from_value(entry.clone()).ok())
        .collect()
}

fn build_mcp_initialize_response(message: &Map<String, Value>, server: &McpServerHandle) -> Value {
    let mut capabilities = Map::new();
    capabilities.insert("tools".into(), Value::Object(Map::new()));

    let mut server_info = Map::new();
    server_info.insert("name".into(), Value::String(server.name().to_string()));
    server_info.insert(
        "version".into(),
        Value::String(server.version().unwrap_or("1.0.0").to_string()),
    );

    let mut result = Map::new();
    result.insert("protocolVersion".into(), Value::String("2024-11-05".into()));
    result.insert("capabilities".into(), Value::Object(capabilities));
    result.insert("serverInfo".into(), Value::Object(server_info));

    let mut response = Map::new();
    response.insert("jsonrpc".into(), Value::String("2.0".into()));
    response.insert(
        "id".into(),
        message.get("id").cloned().unwrap_or(Value::Null),
    );
    response.insert("result".into(), Value::Object(result));
    Value::Object(response)
}

fn convert_mcp_tool_list(tools: Vec<McpToolInfo>) -> Vec<Value> {
    tools
        .into_iter()
        .map(|tool| {
            let mut obj = Map::new();
            obj.insert("name".into(), Value::String(tool.name));
            if let Some(description) = tool.description {
                obj.insert("description".into(), Value::String(description));
            }
            obj.insert(
                "inputSchema".into(),
                tool.input_schema
                    .unwrap_or_else(|| Value::Object(Map::new())),
            );
            Value::Object(obj)
        })
        .collect()
}

fn convert_mcp_call_result(result: McpToolCallResult) -> Value {
    let mut result_map = Map::new();
    let content = result
        .content
        .into_iter()
        .map(|item| match item {
            McpToolContent::Text { text } => json!({ "type": "text", "text": text }),
            McpToolContent::Image { data, mime_type } => {
                json!({ "type": "image", "data": data, "mimeType": mime_type })
            }
            McpToolContent::Json { value } => json!({ "type": "json", "value": value }),
        })
        .collect();
    result_map.insert("content".into(), Value::Array(content));
    if result.is_error {
        result_map.insert("is_error".into(), Value::Bool(true));
    }
    Value::Object(result_map)
}

fn jsonrpc_error(id: Value, code: i64, message: String) -> Value {
    let mut error = Map::new();
    error.insert("code".into(), Value::Number(code.into()));
    error.insert("message".into(), Value::String(message));

    let mut response = Map::new();
    response.insert("jsonrpc".into(), Value::String("2.0".into()));
    response.insert("id".into(), id);
    response.insert("error".into(), Value::Object(error));
    Value::Object(response)
}

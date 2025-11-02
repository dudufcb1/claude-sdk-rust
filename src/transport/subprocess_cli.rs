//! Subprocess-based transport implementation replicating the Python SDK behaviour.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Map, Value};
use tempfile::{NamedTempFile, TempPath};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};

#[cfg(unix)]
use users::get_user_by_name;

use crate::config::{
    AgentDefinition, ClaudeAgentOptions, McpServerConfig, McpServers, SdkPluginKind, SettingSource,
    SystemPrompt,
};
use crate::error::{
    CliConnectionError, CliJsonDecodeError, CliNotFoundError, ProcessError, SdkError,
};
use crate::transport::Transport;

const DEFAULT_MAX_BUFFER_SIZE: usize = 1024 * 1024;
const MINIMUM_CLAUDE_CODE_VERSION: &str = "2.0.0";
#[cfg(windows)]
const CMD_LENGTH_LIMIT: usize = 8_000;
#[cfg(not(windows))]
const CMD_LENGTH_LIMIT: usize = 100_000;

/// Mode describing how the prompt should be handled when starting the CLI.
#[derive(Debug, Clone)]
pub enum PromptMode {
    Text(String),
    Streaming,
}

/// Transport implementation backed by the Claude CLI subprocess.
#[derive(Debug, Clone)]
pub struct SubprocessCliTransport {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    prompt: PromptMode,
    options: ClaudeAgentOptions,
    cli_path: PathBuf,
    cwd: Option<PathBuf>,
    max_buffer_size: usize,
    ready: AtomicBool,
    temp_files: Mutex<Vec<tempfile::TempPath>>,
    child: Mutex<Option<ProcessHandles>>,
    stdout_rx: Mutex<Option<mpsc::Receiver<Result<Value, SdkError>>>>,
    exit_error: Mutex<Option<SdkError>>,
}

#[derive(Debug)]
struct ProcessHandles {
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    stdout_task: JoinHandle<()>,
    stderr_task: Option<JoinHandle<()>>,
}

impl SubprocessCliTransport {
    /// Create a new transport using the provided prompt and options.
    pub fn new(prompt: PromptMode, options: ClaudeAgentOptions) -> Result<Self, SdkError> {
        let cli_path = match &options.cli_path {
            Some(path) => path.clone(),
            None => find_cli()?,
        };

        let cwd = options.cwd.clone();
        let max_buffer_size = options.max_buffer_size.unwrap_or(DEFAULT_MAX_BUFFER_SIZE);

        Ok(Self {
            inner: Arc::new(Inner {
                prompt,
                options,
                cli_path,
                cwd,
                max_buffer_size,
                ready: AtomicBool::new(false),
                temp_files: Mutex::new(Vec::new()),
                child: Mutex::new(None),
                stdout_rx: Mutex::new(None),
                exit_error: Mutex::new(None),
            }),
        })
    }
}

#[async_trait::async_trait]
impl Transport for SubprocessCliTransport {
    async fn connect(&self) -> Result<(), SdkError> {
        {
            let child_guard = self.inner.child.lock().await;
            if child_guard.is_some() {
                return Ok(());
            }
        }

        if std::env::var("CLAUDE_AGENT_SDK_SKIP_VERSION_CHECK").is_err() {
            self.inner.check_version().await?;
        }

        let mut build = self.inner.build_command()?;
        {
            let mut temp_guard = self.inner.temp_files.lock().await;
            temp_guard.extend(build.temp_files.drain(..));
        }

        let mut command = Command::new(&self.inner.cli_path);
        command.args(&build.args);

        if let Some(cwd) = &self.inner.cwd {
            command.current_dir(cwd);
        }

        let mut env: HashMap<String, String> = std::env::vars().collect();
        env.extend(self.inner.options.env.clone());
        env.insert("CLAUDE_CODE_ENTRYPOINT".to_string(), "sdk-rs".to_string());
        env.insert(
            "CLAUDE_AGENT_SDK_VERSION".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );
        if let Some(cwd) = &self.inner.cwd {
            env.insert("PWD".to_string(), cwd.display().to_string());
        }
        for (key, value) in env {
            command.env(key, value);
        }

        let should_pipe_stderr = should_pipe_stderr(&self.inner.options);
        if should_pipe_stderr {
            command.stderr(std::process::Stdio::piped());
        }

        command.stdin(std::process::Stdio::piped());
        command.stdout(std::process::Stdio::piped());

        #[cfg(unix)]
        if let Some(user) = &self.inner.options.user {
            if let Some(info) = get_user_by_name(user) {
                command.uid(info.uid());
                command.gid(info.primary_group_id());
            }
        }

        let mut child = command
            .spawn()
            .map_err(|err| CliConnectionError::new(format!("Failed to start Claude CLI: {err}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CliConnectionError::new("Missing stdout handle from CLI process"))?;
        let stdin = child.stdin.take();
        let stderr = if should_pipe_stderr {
            child.stderr.take()
        } else {
            None
        };

        let child_arc = Arc::new(Mutex::new(child));
        let stdin_arc = Arc::new(Mutex::new(stdin));

        if matches!(self.inner.prompt, PromptMode::Text(_)) {
            log::debug!("[transport::connect] Text prompt mode - closing stdin immediately");
            let mut guard = stdin_arc.lock().await;
            if let Some(mut stdin) = guard.take() {
                let _ = stdin.shutdown().await;
            }
        } else {
            log::debug!("[transport::connect] Streaming mode - keeping stdin open for stream_input");
        }

        let (tx, rx) = mpsc::channel(64);
        let stdout_task =
            spawn_stdout_task(Arc::clone(&self.inner), Arc::clone(&child_arc), stdout, tx);

        let stderr_task = stderr.map(|stream| spawn_stderr_task(Arc::clone(&self.inner), stream));

        {
            let mut child_guard = self.inner.child.lock().await;
            *child_guard = Some(ProcessHandles {
                child: Arc::clone(&child_arc),
                stdin: Arc::clone(&stdin_arc),
                stdout_task,
                stderr_task,
            });
        }

        {
            let mut rx_guard = self.inner.stdout_rx.lock().await;
            *rx_guard = Some(rx);
        }

        self.inner.ready.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn write(&self, payload: &serde_json::Value) -> Result<(), SdkError> {
        if !self.inner.ready.load(Ordering::SeqCst) {
            return Err(SdkError::from(CliConnectionError::new(
                "ProcessTransport is not ready for writing",
            )));
        }

        let line = serde_json::to_string(payload)? + "\n";

        let handles = {
            let child_guard = self.inner.child.lock().await;
            child_guard
                .as_ref()
                .map(|handles| (Arc::clone(&handles.stdin), Arc::clone(&handles.child)))
                .ok_or_else(|| SdkError::from(CliConnectionError::new("Not connected")))?
        };

        {
            let mut stdin_guard = handles.0.lock().await;
            if let Some(stdin) = stdin_guard.as_mut() {
                log::debug!("[transport::write] stdin available, writing {} bytes", line.len());
                stdin.write_all(line.as_bytes()).await.map_err(|err| {
                    CliConnectionError::new(format!("Failed to write to process stdin: {err}"))
                })?;
                stdin.flush().await.map_err(|err| {
                    CliConnectionError::new(format!("Failed to flush process stdin: {err}"))
                })?;
                log::debug!("[transport::write] write successful");
            } else {
                log::error!("[transport::write] stdin is None - was already closed!");
                return Err(SdkError::from(CliConnectionError::new(
                    "Process stdin is not available",
                )));
            }
        }

        let mut child = handles.1.lock().await;
        if let Some(status) = child.try_wait().map_err(|err| {
            CliConnectionError::new(format!("Failed to poll process status: {err}"))
        })? {
            if !status.success() {
                let message = match status.code() {
                    Some(code) => format!("Command failed with exit code {code}"),
                    None => "Command failed with unknown exit status".to_string(),
                };
                return Err(SdkError::from(ProcessError::new(
                    message,
                    status.code(),
                    None,
                )));
            }
        }

        Ok(())
    }

    async fn read(&self) -> Result<Option<serde_json::Value>, SdkError> {
        let mut rx_guard = self.inner.stdout_rx.lock().await;
        let rx = rx_guard
            .as_mut()
            .ok_or_else(|| CliConnectionError::new("Not connected"))?;

        match rx.recv().await {
            Some(Ok(value)) => Ok(Some(value)),
            Some(Err(err)) => Err(err),
            None => {
                let mut exit_error = self.inner.exit_error.lock().await;
                if let Some(err) = exit_error.take() {
                    Err(err)
                } else {
                    Ok(None)
                }
            }
        }
    }

    async fn end_input(&self) -> Result<(), SdkError> {
        log::debug!("[transport::end_input] Called - will close stdin");
        let handles = {
            let child_guard = self.inner.child.lock().await;
            child_guard
                .as_ref()
                .map(|handles| Arc::clone(&handles.stdin))
                .ok_or_else(|| CliConnectionError::new("Not connected"))?
        };

        let mut stdin_guard = handles.lock().await;
        if let Some(mut stdin) = stdin_guard.take() {
            log::debug!("[transport::end_input] Shutting down stdin now");
            stdin
                .shutdown()
                .await
                .map_err(|err| CliConnectionError::new(format!("Failed to close stdin: {err}")))?;
            log::debug!("[transport::end_input] stdin closed successfully");
        } else {
            log::warn!("[transport::end_input] stdin was already None");
        }

        Ok(())
    }

    async fn close(&self) -> Result<(), SdkError> {
        self.inner.ready.store(false, Ordering::SeqCst);

        {
            let mut temp_guard = self.inner.temp_files.lock().await;
            temp_guard.clear();
        }

        let handles = {
            let mut child_guard = self.inner.child.lock().await;
            child_guard.take()
        };

        if let Some(handles) = handles {
            let ProcessHandles {
                child,
                stdin,
                stdout_task,
                stderr_task,
            } = handles;

            if let Some(task) = stderr_task {
                task.abort();
                let _ = task.await;
            }
            stdout_task.abort();
            let _ = stdout_task.await;

            {
                let mut stdin_guard = stdin.lock().await;
                if let Some(mut stdin) = stdin_guard.take() {
                    let _ = stdin.shutdown().await;
                }
            }

            let mut child = child.lock().await;
            if let Ok(None) = child.try_wait() {
                let _ = child.start_kill();
                let _ = timeout(Duration::from_millis(500), child.wait()).await;
            }
        }

        {
            let mut rx_guard = self.inner.stdout_rx.lock().await;
            *rx_guard = None;
        }

        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.inner.ready.load(Ordering::SeqCst)
    }
}

impl Inner {
    fn build_command(&self) -> Result<CommandBuild, SdkError> {
        let mut args: Vec<OsString> = Vec::new();
        args.push(OsString::from("--output-format"));
        args.push(OsString::from("stream-json"));
        args.push(OsString::from("--verbose"));

        match &self.options.system_prompt {
            None => {
                args.push(OsString::from("--system-prompt"));
                args.push(OsString::from(""));
            }
            Some(SystemPrompt::Text(text)) => {
                args.push(OsString::from("--system-prompt"));
                args.push(text.clone().into());
            }
            Some(SystemPrompt::Preset(preset)) => {
                if let Some(append) = &preset.append {
                    args.push(OsString::from("--append-system-prompt"));
                    args.push(append.clone().into());
                }
            }
        }

        if !self.options.allowed_tools.is_empty() {
            args.push(OsString::from("--allowedTools"));
            args.push(self.options.allowed_tools.join(",").into());
        }

        if let Some(max_turns) = self.options.max_turns {
            args.push(OsString::from("--max-turns"));
            args.push(max_turns.to_string().into());
        }

        if let Some(max_budget) = self.options.max_budget_usd {
            args.push(OsString::from("--max-budget-usd"));
            args.push(max_budget.to_string().into());
        }

        if !self.options.disallowed_tools.is_empty() {
            args.push(OsString::from("--disallowedTools"));
            args.push(self.options.disallowed_tools.join(",").into());
        }

        if let Some(model) = &self.options.model {
            args.push(OsString::from("--model"));
            args.push(model.clone().into());
        }

        if let Some(tool_name) = &self.options.permission_prompt_tool_name {
            args.push(OsString::from("--permission-prompt-tool"));
            args.push(tool_name.clone().into());
        }

        if let Some(mode) = &self.options.permission_mode {
            args.push(OsString::from("--permission-mode"));
            args.push(mode.as_str().into());
        }

        if self.options.continue_conversation {
            args.push(OsString::from("--continue"));
        }

        if let Some(resume) = &self.options.resume {
            args.push(OsString::from("--resume"));
            args.push(resume.clone().into());
        }

        if let Some(settings) = &self.options.settings {
            args.push(OsString::from("--settings"));
            args.push(settings.clone().into());
        }

        for directory in &self.options.add_dirs {
            args.push(OsString::from("--add-dir"));
            args.push(directory.display().to_string().into());
        }

        let has_mcp_servers = match &self.options.mcp_servers {
            McpServers::Map(map) => !map.is_empty(),
            McpServers::Inline(value) => !value.trim().is_empty(),
            McpServers::Path(_) => true,
        };

        if has_mcp_servers {
            let mcp_arg = build_mcp_argument(&self.options.mcp_servers)?;
            args.push(OsString::from("--mcp-config"));
            args.push(mcp_arg.into());
        }

        if self.options.include_partial_messages {
            args.push(OsString::from("--include-partial-messages"));
        }

        if self.options.fork_session {
            args.push(OsString::from("--fork-session"));
        }

        if let Some(agents) = &self.options.agents {
            if !agents.is_empty() {
                let agents_json = build_agents_json(agents)?;
                args.push(OsString::from("--agents"));
                args.push(agents_json.into());
            }
        }

        let sources_value = self
            .options
            .setting_sources
            .as_ref()
            .map(|sources| {
                sources
                    .iter()
                    .map(SettingSource::as_str)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        args.push(OsString::from("--setting-sources"));
        args.push(sources_value.into());

        for plugin in &self.options.plugins {
            match plugin.kind {
                SdkPluginKind::Local => {
                    args.push(OsString::from("--plugin-dir"));
                    args.push(plugin.path.display().to_string().into());
                }
            }
        }

        for (flag, value) in &self.options.extra_args {
            let flag_name = format!("--{flag}");
            args.push(flag_name.into());
            if let Some(value) = value {
                args.push(value.clone().into());
            }
        }

        match &self.prompt {
            PromptMode::Streaming => {
                args.push(OsString::from("--input-format"));
                args.push(OsString::from("stream-json"));
            }
            PromptMode::Text(prompt) => {
                args.push(OsString::from("--print"));
                args.push(OsString::from("--"));
                args.push(prompt.clone().into());
            }
        }

        if let Some(max_thinking) = self.options.max_thinking_tokens {
            args.push(OsString::from("--max-thinking-tokens"));
            args.push(max_thinking.to_string().into());
        }

        let mut temp_files: Vec<TempPath> = Vec::new();
        let cmd_len = command_length(&self.cli_path, &args);
        if cmd_len > CMD_LENGTH_LIMIT {
            if let Some(position) = args.iter().position(|arg| arg == "--agents") {
                if position + 1 < args.len() {
                    let agents_json = args[position + 1].to_string_lossy().to_string();
                    let mut temp_file = NamedTempFile::new()?;
                    temp_file.write_all(agents_json.as_bytes())?;
                    let temp_path = temp_file.into_temp_path();
                    let replacement = format!("@{}", temp_path.display());
                    args[position + 1] = replacement.into();
                    temp_files.push(temp_path);
                }
            }
        }

        Ok(CommandBuild { args, temp_files })
    }

    async fn check_version(&self) -> Result<(), SdkError> {
        let output = match timeout(
            Duration::from_secs(2),
            Command::new(&self.cli_path).arg("-v").output(),
        )
        .await
        {
            Ok(Ok(output)) => output,
            _ => return Ok(()),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        if let (Some(current), Some(minimum)) = (
            parse_version_components(&stdout),
            parse_version_components(MINIMUM_CLAUDE_CODE_VERSION),
        ) {
            if current < minimum {
                eprintln!(
                    "Warning: Claude Code version {} is unsupported. Minimum required version is {MINIMUM_CLAUDE_CODE_VERSION}.",
                    stdout.trim()
                );
            }
        }

        Ok(())
    }
}

struct CommandBuild {
    args: Vec<OsString>,
    temp_files: Vec<TempPath>,
}

fn find_cli() -> Result<PathBuf, SdkError> {
    if let Ok(path) = which::which("claude") {
        return Ok(path);
    }

    let home = dirs::home_dir();
    let mut locations: Vec<PathBuf> = Vec::new();

    if let Some(ref home_dir) = home {
        locations.push(home_dir.join(".npm-global/bin/claude"));
        locations.push(home_dir.join(".local/bin/claude"));
        locations.push(home_dir.join("node_modules/.bin/claude"));
        locations.push(home_dir.join(".yarn/bin/claude"));
        locations.push(home_dir.join(".claude/local/claude"));
    }

    locations.push(PathBuf::from("/usr/local/bin/claude"));

    for path in locations {
        if path.exists() && path.is_file() {
            return Ok(path);
        }
    }

    Err(SdkError::from(CliNotFoundError::new(
        "Claude Code not found. Install with:\n  npm install -g @anthropic-ai/claude-code\n\nIf already installed locally, try:\n  export PATH=\"$HOME/node_modules/.bin:$PATH\"\n\nOr provide the path via ClaudeAgentOptions(cli_path=...)",
        None,
    )))
}

fn build_mcp_argument(servers: &McpServers) -> Result<String, SdkError> {
    match servers {
        McpServers::Inline(inline) => Ok(inline.clone()),
        McpServers::Path(path) => Ok(path.display().to_string()),
        McpServers::Map(map) => {
            let mut mcp_servers = Map::new();
            for (name, config) in map {
                let value = match config {
                    McpServerConfig::Sdk(sdk) => {
                        let mut sdk_value = serde_json::to_value(sdk)?;
                        if let Value::Object(ref mut obj) = sdk_value {
                            obj.remove("instance");
                        }
                        sdk_value
                    }
                    _ => serde_json::to_value(config)?,
                };
                mcp_servers.insert(name.clone(), value);
            }
            Ok(serde_json::to_string(
                &json!({ "mcpServers": mcp_servers }),
            )?)
        }
    }
}

fn build_agents_json(agents: &HashMap<String, AgentDefinition>) -> Result<String, SdkError> {
    let mut root = Map::new();
    for (name, agent) in agents {
        let mut value = serde_json::to_value(agent)?
            .as_object()
            .cloned()
            .unwrap_or_default();
        value.retain(|_, v| !v.is_null());
        root.insert(name.clone(), Value::Object(value));
    }
    Ok(serde_json::to_string(&Value::Object(root))?)
}

fn should_pipe_stderr(options: &ClaudeAgentOptions) -> bool {
    options.stderr.is_some() || options.extra_args.contains_key("debug-to-stderr")
}

fn command_length(cli_path: &Path, args: &[OsString]) -> usize {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(cli_path.to_string_lossy().to_string());
    parts.extend(args.iter().map(|arg| arg.to_string_lossy().to_string()));
    parts.join(" ").len()
}

fn parse_version_components(input: &str) -> Option<[u32; 3]> {
    let token = input
        .split_whitespace()
        .find(|segment| segment.chars().all(|ch| ch.is_ascii_digit() || ch == '.'))?;

    let mut parts = token.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some([major, minor, patch])
}

fn spawn_stdout_task(
    inner: Arc<Inner>,
    child: Arc<Mutex<Child>>,
    stdout: ChildStdout,
    sender: mpsc::Sender<Result<Value, SdkError>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut buffer = String::new();
        let mut json_buffer = String::new();

        loop {
            buffer.clear();
            match reader.read_line(&mut buffer).await {
                Ok(0) => break,
                Ok(_) => {
                    for fragment in buffer.split('\n') {
                        let fragment = fragment.trim();
                        if fragment.is_empty() {
                            continue;
                        }
                        json_buffer.push_str(fragment);
                        if json_buffer.len() > inner.max_buffer_size {
                            let err_message = format!(
                                "Buffer size {} exceeds limit {}",
                                json_buffer.len(),
                                inner.max_buffer_size
                            );
                            let snapshot = json_buffer.clone();

                            let send_error = CliJsonDecodeError::new(
                                snapshot.clone(),
                                serde_json::Error::io(std::io::Error::new(
                                    ErrorKind::InvalidData,
                                    err_message.clone(),
                                )),
                            );
                            let _ = sender.send(Err(SdkError::from(send_error))).await;

                            let stored_error = CliJsonDecodeError::new(
                                snapshot,
                                serde_json::Error::io(std::io::Error::new(
                                    ErrorKind::InvalidData,
                                    err_message,
                                )),
                            );
                            *inner.exit_error.lock().await = Some(SdkError::from(stored_error));

                            json_buffer.clear();
                            continue;
                        }
                        match serde_json::from_str::<Value>(&json_buffer) {
                            Ok(value) => {
                                json_buffer.clear();
                                if sender.send(Ok(value)).await.is_err() {
                                    return;
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                }
                Err(err) => {
                    let _ = sender
                        .send(Err(SdkError::from(CliConnectionError::new(format!(
                            "Failed to read stdout: {err}"
                        )))))
                        .await;
                    return;
                }
            }
        }

        let status = {
            let mut child_guard = child.lock().await;
            child_guard.wait().await
        };

        match status {
            Ok(status) => {
                if !status.success() {
                    let error = ProcessError::new(
                        match status.code() {
                            Some(code) => format!("Command failed with exit code {code}"),
                            None => "Command failed with unknown exit status".to_string(),
                        },
                        status.code(),
                        None,
                    );
                    *inner.exit_error.lock().await = Some(SdkError::from(error.clone()));
                    let _ = sender.send(Err(SdkError::from(error))).await;
                }
            }
            Err(err) => {
                let error = CliConnectionError::new(format!("Failed to wait for process: {err}"));
                *inner.exit_error.lock().await = Some(SdkError::from(error.clone()));
                let _ = sender.send(Err(SdkError::from(error))).await;
            }
        }

        drop(sender);
    })
}

fn spawn_stderr_task(inner: Arc<Inner>, stderr: ChildStderr) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        while reader
            .read_line(&mut line)
            .await
            .ok()
            .filter(|len| *len > 0)
            .is_some()
        {
            let text = line.trim_end().to_string();
            line.clear();
            if text.is_empty() {
                continue;
            }
            if let Some(callback) = inner.options.stderr.as_ref() {
                callback(&text);
            } else if inner.options.extra_args.contains_key("debug-to-stderr") {
                if let Some(callback) = inner.options.debug_stderr.as_ref() {
                    callback(&text);
                }
            }
        }
    })
}

impl SettingSource {
    fn as_str(&self) -> &'static str {
        match self {
            SettingSource::User => "user",
            SettingSource::Project => "project",
            SettingSource::Local => "local",
        }
    }
}

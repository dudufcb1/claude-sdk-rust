# Claude SDK for Rust

![license](https://img.shields.io/badge/license-MIT-blue.svg)
![rustc](https://img.shields.io/badge/rustc-1.75%2B-9347FF.svg)
![status](https://img.shields.io/badge/status-experimental-orange.svg)

Rust-native SDK for automating workflows against the Claude Code CLI. This crate mirrors the official Python SDK surface while respecting idiomatic async Rust patterns.

## Features

- Async client (`ClaudeSdkClient`) with streaming and one-shot query entry points.
- Comprehensive message model covering user, assistant, system, tool-use and control messages.
- Built-in transport that manages the Claude Code subprocess lifecycle.
- MCP (Model Context Protocol) server helpers for registering in-process tools.
- Permission hooks, stderr callbacks, and partial message support consistent with the CLI UX.

## Getting Started

### Prerequisites

- Rust 1.75 or newer (via [rustup](https://rustup.rs/)).
- Claude Code CLI installed and accessible on `PATH`.
- `ANTHROPIC_API_KEY` scoped for the environments you plan to target.

### Add as a dependency

Until crates.io publication, pull directly from GitHub:

```toml
[dependencies]
sdk_claude_rust = { git = "https://github.com/dudufcb1/claude-sdk-rust.git", branch = "main" }
```

Or for a local checkout:

```toml
[dependencies]
sdk_claude_rust = { path = "../sdk-claude-rust" }
```

### Quick example

```rust
use sdk_claude_rust::client::ClaudeSdkClient;
use sdk_claude_rust::query::query;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Simple streaming query using the high level helper
    let mut stream = query("Explain Rust ownership in two sentences.", None, None).await?;

    while let Some(chunk) = stream.next().await {
        match chunk? {
            sdk_claude_rust::message::Message::Assistant(msg) => {
                for block in msg.content {
                    if let sdk_claude_rust::message::ContentBlock::Text(text) = block {
                        println!("{}", text.text);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}
```

See [`examples/`](examples/) for full recipes (hooks, agents, plugins, streaming control, partial messages, etc.).

## Permissions and directory access

Claude Code enforces a permission system for tool usage and filesystem access. Configure it via `ClaudeAgentOptions`:

- Set a session permission mode via `ClaudeAgentOptions.permission_mode` (see `PermissionMode` in `src/permission.rs`).
- Allow additional directories for tool access via `ClaudeAgentOptions.add_dirs` (maps to repeated `--add-dir`).
- Change the CLI working directory via `ClaudeAgentOptions.cwd`.
- Pass raw Claude Code CLI flags via `ClaudeAgentOptions.extra_args` (maps to `--{flag}` with optional value).

Example (bypass prompts and allow a repo directory):

```rust
use std::path::PathBuf;

use sdk_claude_rust::config::ClaudeAgentOptions;
use sdk_claude_rust::permission::PermissionMode;

let options = ClaudeAgentOptions {
    permission_mode: Some(PermissionMode::BypassPermissions),
    add_dirs: vec![PathBuf::from("/home/eduardo/n8ngod")],
    ..Default::default()
};
```

Example (pass CLI flags not exposed by the Rust types):

```rust
use std::collections::HashMap;

use sdk_claude_rust::config::ClaudeAgentOptions;

let mut extra_args = HashMap::new();
extra_args.insert("dangerously-skip-permissions".to_string(), None);
extra_args.insert("permission-mode".to_string(), Some("dontAsk".to_string()));

let options = ClaudeAgentOptions {
    permission_mode: None,
    extra_args,
    ..Default::default()
};
```

## Tests and Quality Gates

- `cargo fmt`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `cargo check --examples`
- End-to-end flows: `cargo test -- --ignored` (requires `ANTHROPIC_API_KEY`, local Claude CLI)

## Project Structure

```
src/                # Library entry points and public API
src/internal/       # Ported internals mirroring the Python SDK (_internal)
src/transport/      # Subprocess transport and CLI orchestration
examples/           # Parity samples with the Python SDK
scripts/            # Tooling (pre-push hook, setup helpers)
tests/              # Unit, integration, and e2e harnesses
```

## License

Licensed under the [MIT License](LICENSE).

//! Environment configuration helpers for loading API credentials from .env files.

use std::collections::HashMap;
use std::path::Path;

/// Loads environment variables from a .env file in the specified directory.
/// Falls back to the current directory if no path is provided.
///
/// This function loads variables into the process environment and returns
/// a HashMap suitable for passing to `ClaudeAgentOptions.env`.
///
/// # Example
/// ```no_run
/// use sdk_claude_rust::env::load_env;
///
/// // Load from current directory
/// let env_vars = load_env(None).unwrap();
///
/// // Load from specific path
/// let env_vars = load_env(Some("/path/to/project")).unwrap();
/// ```
pub fn load_env(dir: Option<&Path>) -> Result<HashMap<String, String>, EnvError> {
    let env_path = match dir {
        Some(d) => d.join(".env"),
        None => std::env::current_dir()
            .map_err(|e| EnvError::Io(e.to_string()))?
            .join(".env"),
    };

    if env_path.exists() {
        dotenvy::from_path(&env_path).map_err(|e| EnvError::Parse(e.to_string()))?;
    }

    Ok(get_anthropic_env())
}

/// Returns a HashMap with ANTHROPIC_* environment variables.
/// Use this to pass credentials to ClaudeAgentOptions.env.
pub fn get_anthropic_env() -> HashMap<String, String> {
    let mut env = HashMap::new();

    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        env.insert("ANTHROPIC_API_KEY".to_string(), key);
    }

    if let Ok(url) = std::env::var("ANTHROPIC_BASE_URL") {
        env.insert("ANTHROPIC_BASE_URL".to_string(), url);
    }

    if let Ok(model) = std::env::var("ANTHROPIC_MODEL") {
        env.insert("ANTHROPIC_MODEL".to_string(), model);
    }

    env
}

/// Creates ClaudeAgentOptions with environment variables loaded from .env.
/// This is a convenience function that combines load_env with options creation.
///
/// # Example
/// ```no_run
/// use sdk_claude_rust::env::options_from_env;
///
/// let options = options_from_env(None).unwrap();
/// // options.env now contains ANTHROPIC_API_KEY, ANTHROPIC_BASE_URL, etc.
/// ```
pub fn options_from_env(dir: Option<&Path>) -> Result<crate::config::ClaudeAgentOptions, EnvError> {
    let env_vars = load_env(dir)?;

    let mut options = crate::config::ClaudeAgentOptions::default();
    options.env = env_vars;

    // Also set model if provided
    if let Ok(model) = std::env::var("ANTHROPIC_MODEL") {
        options.model = Some(model);
    }

    Ok(options)
}

/// Errors that can occur when loading environment configuration.
#[derive(Debug, Clone)]
pub enum EnvError {
    Io(String),
    Parse(String),
}

impl std::fmt::Display for EnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvError::Io(msg) => write!(f, "IO error: {}", msg),
            EnvError::Parse(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for EnvError {}

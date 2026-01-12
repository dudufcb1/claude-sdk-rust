#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOOKS_DIR="$REPO_ROOT/.git/hooks"
SCRIPT_DIR="$REPO_ROOT/scripts"

echo "=== Claude SDK Rust - Setup ==="
echo ""

# Check Rust
if ! command -v cargo &> /dev/null; then
    echo "[ERROR] Rust not found. Install from https://rustup.rs"
    exit 1
fi
echo "[OK] Rust: $(cargo --version)"

# Check Node.js
if ! command -v node &> /dev/null; then
    echo "[ERROR] Node.js not found. Install Node.js 20+ from https://nodejs.org"
    exit 1
fi

NODE_VERSION=$(node -v | cut -d'v' -f2 | cut -d'.' -f1)
if [ "$NODE_VERSION" -lt 20 ]; then
    echo "[WARN] Node.js version is $NODE_VERSION. Version 20+ recommended."
else
    echo "[OK] Node.js: $(node -v)"
fi

# Check Claude CLI
if ! command -v claude &> /dev/null; then
    echo "[WARN] Claude CLI not found."
    read -p "Install now with npm? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        npm install -g @anthropic-ai/claude-code
        echo "[OK] Claude CLI installed"
    else
        echo "[SKIP] Install manually: npm install -g @anthropic-ai/claude-code"
    fi
else
    echo "[OK] Claude CLI: $(claude -v 2>/dev/null || echo 'installed')"
fi

# Create .env if not exists
if [ ! -f "$REPO_ROOT/.env" ]; then
    if [ -f "$REPO_ROOT/.env.example" ]; then
        cp "$REPO_ROOT/.env.example" "$REPO_ROOT/.env"
        echo "[OK] Created .env from .env.example"
        echo "     Edit .env with your ANTHROPIC_API_KEY and ANTHROPIC_BASE_URL"
    fi
else
    echo "[OK] .env already exists"
fi

# Install git hooks
mkdir -p "$HOOKS_DIR"
if [ -f "$SCRIPT_DIR/pre-push" ]; then
    cp "$SCRIPT_DIR/pre-push" "$HOOKS_DIR/pre-push"
    chmod +x "$HOOKS_DIR/pre-push"
    echo "[OK] Installed pre-push hook"
fi

# Verify build
echo ""
echo "=== Verifying build ==="
cd "$REPO_ROOT"
if cargo check 2>/dev/null; then
    echo "[OK] Project compiles successfully"
else
    echo "[ERROR] Build failed. Check dependencies."
    exit 1
fi

echo ""
echo "=== Setup complete! ==="
echo ""
echo "Next steps:"
echo "  1. Edit .env with your API credentials"
echo "  2. Run an example: cargo run --example mcp_calculator"
echo ""

#!/usr/bin/env bash
# Build (release) and run cardano-observer.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

# rustup's cargo is not on systemd's minimal PATH
if [ -f "${HOME}/.cargo/env" ]; then
  # shellcheck source=/dev/null
  source "${HOME}/.cargo/env"
fi

if [ ! -f .env ]; then
  echo "No .env found - copying .env.example to .env."
  echo "Edit .env with your Ogmios/Blockfrost host info before pointing this at a real node."
  cp .env.example .env
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found - install Rust (https://rustup.rs) or set PATH in the systemd unit." >&2
  exit 127
fi

# Static UI is baked into the binary via include_str! - always rebuild this
# package so copying a fresh static/ can't leave an old HTML/JS/CSS embedded.
echo "Cleaning cardano-observer package..."
cargo clean -p cardano-observer

echo "Building release binary..."
cargo build --release

echo "Starting cardano-observer..."
exec ./target/release/cardano-observer

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
  echo "Edit .env with your Ogmios / db-sync host info before pointing this at a real node."
  cp .env.example .env
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found - install Rust (https://rustup.rs) or set PATH in the systemd unit." >&2
  exit 127
fi

# Read a config value: prefer an exported env var, else fall back to .env.
# Mirrors how the binaries resolve config (real env wins over .env).
env_val() {
  local key="$1" val="${!1-}"
  if [ -n "${val}" ]; then printf '%s' "${val}"; return; fi
  if [ -f .env ]; then
    val="$(grep -E "^[[:space:]]*${key}=" .env | tail -n1 || true)"
    val="${val#*=}"
    val="${val%$'\r'}"
    case "${val}" in
      \"*\") val="${val#\"}"; val="${val%\"}" ;;
      \'*\') val="${val#\'}"; val="${val%\'}" ;;
    esac
    printf '%s' "${val}"
  fi
}

# Static UI is baked into the binary via include_str! - always rebuild this
# package so copying a fresh static/ can't leave an old HTML/JS/CSS embedded.
echo "Cleaning cardano-observer package..."
cargo clean -p cardano-observer

echo "Building release binary..."
cargo build --release -p cardano-observer

# Build and start the data backend alongside the observer when a local db-sync
# is configured (DBSYNC_URL). If DBSYNC_URL is unset, the backend is assumed to
# run elsewhere (e.g. on the db-sync host) and only OBSERVER_BACKEND_URL is
# used - so we build/launch nothing here.
backend_pid=""
if [ -n "$(env_val DBSYNC_URL)" ]; then
  echo "Building cardano-observer-backend..."
  cargo build --release -p cardano-observer-backend
  echo "Starting cardano-observer-backend..."
  ./target/release/cardano-observer-backend >>backend.log 2>&1 &
  backend_pid=$!
  echo "  backend pid ${backend_pid}, logging to $(pwd)/backend.log"
  trap '[ -n "${backend_pid}" ] && kill "${backend_pid}" 2>/dev/null || true' EXIT INT TERM
  # Point the observer at the backend we just launched, unless already set.
  if [ -z "$(env_val OBSERVER_BACKEND_URL)" ]; then
    backend_port="$(env_val BACKEND_BIND)"; backend_port="${backend_port##*:}"
    export OBSERVER_BACKEND_URL="http://127.0.0.1:${backend_port:-3300}"
    echo "  observer will use OBSERVER_BACKEND_URL=${OBSERVER_BACKEND_URL}"
  fi
elif [ -n "$(env_val OBSERVER_BACKEND_URL)" ]; then
  echo "DBSYNC_URL unset - using the backend at $(env_val OBSERVER_BACKEND_URL);"
  echo "not starting one locally."
else
  echo "No backend configured (DBSYNC_URL and OBSERVER_BACKEND_URL both empty)."
  echo "Running from Ogmios + on-disk caches only - enrichment will be limited."
fi

echo "Starting cardano-observer..."
# Not exec'd when a backend is running so the trap can stop it on exit.
if [ -n "${backend_pid}" ]; then
  ./target/release/cardano-observer
else
  exec ./target/release/cardano-observer
fi

#!/usr/bin/env bash
# Dev mode: rebuild and restart automatically whenever source or static files
# change (src/**, static/**, Cargo.toml). Since the frontend is embedded in
# the binary via include_str!, a rebuild is required to pick up HTML/CSS/JS
# edits too - cargo-watch handles that; just refresh your browser after each
# rebuild.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

if [ ! -f .env ]; then
  echo "No .env found - copying .env.example to .env."
  echo "Edit .env with your Ogmios / db-sync host info, or set DEMO=true to try it without a live node."
  cp .env.example .env
fi

if ! command -v cargo-watch >/dev/null 2>&1; then
  echo "cargo-watch not found - installing it once (cargo install cargo-watch)..."
  cargo install cargo-watch
fi

# Read a config value: prefer an exported env var, else fall back to .env.
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

# Run the data backend alongside the watch loop when a local db-sync is
# configured (DBSYNC_URL). A remote backend (DBSYNC_URL unset) is left alone.
# The backend is built once and not watched - restart this script after editing
# backend source.
backend_pid=""
if [ -n "$(env_val DBSYNC_URL)" ]; then
  echo "Building cardano-observer-backend..."
  cargo build -p cardano-observer-backend
  echo "Starting cardano-observer-backend..."
  ./target/debug/cardano-observer-backend >>backend.log 2>&1 &
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

echo "Watching src/, static/, and Cargo.toml - rebuilding and restarting on change."
# Not exec'd when a backend is running so the trap can stop it on exit.
cargo watch \
  --watch src \
  --watch static \
  --watch Cargo.toml \
  --clear \
  -x "run -p cardano-observer"

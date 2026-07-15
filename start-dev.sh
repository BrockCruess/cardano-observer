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
  echo "Edit .env with your Ogmios/Blockfrost host info, or set DEMO=true to try it without a live node."
  cp .env.example .env
fi

if ! command -v cargo-watch >/dev/null 2>&1; then
  echo "cargo-watch not found - installing it once (cargo install cargo-watch)..."
  cargo install cargo-watch
fi

echo "Watching src/, static/, and Cargo.toml - rebuilding and restarting on change."
exec cargo watch \
  --watch src \
  --watch static \
  --watch Cargo.toml \
  --clear \
  -x run

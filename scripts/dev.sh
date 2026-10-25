#!/usr/bin/env bash
# Builds the browser client and runs a server you can play on: open http://127.0.0.1:8080 and
# the server's bots take the nine other seats after EL_FILL seconds (3 by default).
#   scripts/dev.sh                 # webgl2 build
#   EL_FILL=30 scripts/dev.sh      # wait for friends first
# More agents: cargo run -p el-bot --release --bin el-swarm -- --bots 9
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/build-web.sh "${EL_GFX:-webgl2}"
cargo build --release -p el-server
exec "${CARGO_TARGET_DIR:-target}/release/el-server" --web-dir web/dist --fill-after "${EL_FILL:-3}" "$@"

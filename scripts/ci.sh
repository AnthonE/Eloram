#!/usr/bin/env bash
# Everything CI checks, in order. E2E runs only when EL_E2E=1 (needs Chromium; see e2e/).
set -euo pipefail
cd "$(dirname "$0")/.."
step() { echo; echo "==> $*"; }

step "format";                cargo fmt --all -- --check
step "clippy (native)";       cargo clippy --workspace --all-targets -- -D warnings
step "clippy (wasm client)";  cargo clippy -p el-client --target wasm32-unknown-unknown -- -D warnings
step "clippy (wasm, webgpu)"; cargo clippy -p el-client --target wasm32-unknown-unknown --features webgpu -- -D warnings
step "tests";                 cargo test --workspace --release
if [ "${EL_E2E:-0}" = "1" ]; then
  step "web build";           ./scripts/build-web.sh webgl2
  step "e2e: transport";      ./scripts/e2e.sh spike webgl2
fi
echo; echo "all green"

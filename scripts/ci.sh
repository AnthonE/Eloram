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
step "determinism on wasm";   cargo test -p el-sim --target wasm32-unknown-unknown --release --test determinism
# The desktop client's prediction runs without glam's scalar maths (Bevy's glam won't take them):
# the same goldens must hold without them.
step "determinism, no scalar maths"; cargo test -p el-sim --release --test determinism
# The desktop client (needs libudev-dev, libwayland-dev, libasound2-dev, libxkbcommon-dev). Never
# in one cargo run with el-server or el-bot, which turn scalar maths on.
if [ "${EL_DESKTOP:-0}" = "1" ]; then
  step "clippy (desktop)";    cargo clippy -p el-client --features desktop --all-targets -- -D warnings
  step "tests (desktop)";     cargo test -p el-client --features desktop --lib
fi
if [ "${EL_E2E:-0}" = "1" ]; then
  step "web build";           ./scripts/build-web.sh webgl2
  step "e2e: transport";      ./scripts/e2e.sh spike webgl2
  step "e2e: a match";        ./scripts/e2e.sh match webgl2
  step "e2e: the main menu";  ./scripts/e2e.sh menu webgl2
  step "e2e: hitches";        ./scripts/e2e.sh hitches webgl2
  step "e2e: watching";       ./scripts/e2e.sh watch webgl2
  step "e2e: sign-in";        ./scripts/e2e.sh login webgl2
fi
echo; echo "all green"

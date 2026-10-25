#!/usr/bin/env bash
# Runs a Playwright suite against a freshly started server.
#   scripts/e2e.sh spike [project]   # transport smoke test: echo, then the game's Hello and Welcome
# Anything after the project goes to Playwright.
# The server listens on EL_HTTP_PORT (8080) and EL_WT_PORT (UDP, 4433).
set -euo pipefail
cd "$(dirname "$0")/.."
suite="${1:-spike}"
project="${2:-webgl2}"
port="${EL_HTTP_PORT:-8080}"
export EL_URL="http://127.0.0.1:${port}"
# The dev container keeps Chromium in /opt/pw-browsers; elsewhere it's where `playwright install` put it.
if [ -z "${PLAYWRIGHT_BROWSERS_PATH:-}" ] && [ -d /opt/pw-browsers ]; then
  export PLAYWRIGHT_BROWSERS_PATH=/opt/pw-browsers
fi

cargo build --release -p el-server
bin="${CARGO_TARGET_DIR:-target}/release"
wt="${EL_WT_PORT:-4433}"
server() { "$bin/el-server" --web-dir web/dist "$@"; }
pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done; }
trap cleanup EXIT

case "$suite" in
  spike)
    # One server bounces datagrams back; the other is the game, for the Hello and its Welcome.
    server --mode echo --http "127.0.0.1:${port}" --wt-port "$wt" &
    pids+=($!)
    server --mode game --http "127.0.0.1:$((port + 1))" --wt-port "$((wt + 1))" &
    pids+=($!)
    export EL_GAME_URL="http://127.0.0.1:$((port + 1))"
    ;;
  *) echo "unknown suite $suite" >&2; exit 1 ;;
esac

# Wait for the HTTP sides.
for url in "$EL_URL" "${EL_GAME_URL:-$EL_URL}"; do
  for _ in $(seq 1 100); do
    curl -sf "$url/status" >/dev/null && break
    sleep 0.1
  done
done

cd e2e
[ -d node_modules ] || npm ci --no-audit --no-fund
shift 2 || true
npx playwright test "tests/$suite.spec.ts" --project "$project" "$@"

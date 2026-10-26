#!/usr/bin/env bash
# Runs a Playwright suite against a freshly started server.
#   scripts/e2e.sh spike [project]   # transport smoke test: echo, then the game's Hello and Welcome
#   scripts/e2e.sh match [project]   # a match from the queue against the server's bots
#   scripts/e2e.sh watch [project]   # an agent's match watched live from its record, then replayed
#   scripts/e2e.sh login [project]   # wallet sign-in with a stub wallet, /online.json, the resume token
#   scripts/e2e.sh art [project]     # the map's look: the camera over each place, pictures in artifacts/
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
# `exec`, so the pid kept is the server's own and killing it stops the server.
server() { exec "$bin/el-server" --web-dir web/dist "$@"; }
pids=()
# Each server is waited for, so the next suite's can have its ports.
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; done; }
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
  match|art)
    # Bots take the empty seats a second after the first player queues. Champion select runs 45 s,
    # not 20 (it ends once everyone is ready): software rendering stalls on its first frames.
    server --mode game --fill-after 1 --select-secs 45 --http "127.0.0.1:${port}" --wt-port "$wt" &
    pids+=($!)
    ;;
  watch)
    # An agent plays a match for the page to watch.
    cargo build --release -p el-bot
    server --mode game --fill-after 1 --http "127.0.0.1:${port}" --wt-port "$wt" &
    pids+=($!)
    (sleep 2; exec "$bin/el-swarm" --server "$EL_URL" --bots 1 --secs 600 >/dev/null) &
    pids+=($!)
    ;;
  login)
    # The stub wallet signs through el-auth's example with a throwaway key.
    cargo build --release -p el-auth --example sign
    server --mode game --http "127.0.0.1:${port}" --wt-port "$wt" &
    pids+=($!)
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
# The suite and the project, as many of them as were given.
shift $(( $# < 2 ? $# : 2 ))
npx playwright test "tests/$suite.spec.ts" --project "$project" "$@"

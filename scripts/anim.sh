#!/usr/bin/env bash
# The anim stage (`docs/CHAMPIONS.md`, "The stage"): one champion doing one thing, twice (in
# profile over a dark card, three-quarters over a light one), on the fx stage's clock that steps
# 1/60 s, a frame a step, composed into a strip with its numbers and a diff against the last run
# (`el-client/src/animstage.rs`, `el_client_core::anim`, `e2e/fxstrip.mjs`).
#
#   scripts/anim.sh list [filter]                    # every champion's every act
#   scripts/anim.sh knobs                            # the motion's knobs, with the game's values
#   scripts/anim.sh vire-attack seren-r              # → e2e/artifacts/anim/<name>/{strip.png,numbers.json,diff.png}
#   scripts/anim.sh vire-attack --knobs 'snap=0.3;hold_melee=0.1'   # retune, no rebuild
#   scripts/anim.sh vire-attack --frames 24 --span 1.2              # more frames, a shorter span
#   scripts/anim.sh vire-attack --steps 0,16,20,24,28               # exactly these steps
#   scripts/anim.sh --no-build …                     # the page as it was last built
#   EL_ANIM_DIST=dir scripts/anim.sh --no-build …    # a copy of web/dist, while web/dist is rebuilt
#
# Builds the WebGL2 page and serves it itself: no game server. Headless Chromium draws in software,
# a frame every second or two: a strip of fifteen frames takes a few minutes.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=${EL_ANIM_OUT:-e2e/artifacts/anim}
DIST=${EL_ANIM_DIST:-web/dist}
BUILD=1
NAMES=()
KNOBS=""
QUALITY=high
MORE=()
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --knobs) KNOBS=$2; shift ;;
    --low) QUALITY=low ;;
    --frames|--span|--steps|--timeout|--size) MORE+=("$1" "$2"); shift ;;
    -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
    *) NAMES+=("$1") ;;
  esac
  shift
done
[ ${#NAMES[@]} -gt 0 ] || { sed -n '2,17p' "$0"; exit 2; }

case "${NAMES[0]}" in
  list) cargo run -q -p el-client-core --example animstage -- list "${NAMES[@]:1}"; exit 0 ;;
  knobs) cargo run -q -p el-client-core --example animstage -- knobs; exit 0 ;;
esac

if [ "$BUILD" = 1 ]; then
  ./scripts/build-web.sh webgl2
fi
[ -f "$DIST/webgl2/el_bg.wasm" ] || { echo "anim: no page in $DIST: run without --no-build" >&2; exit 1; }
if [ -z "${PLAYWRIGHT_BROWSERS_PATH:-}" ] && [ -d /opt/pw-browsers ]; then
  export PLAYWRIGHT_BROWSERS_PATH=/opt/pw-browsers
fi
[ -d e2e/node_modules ] || (cd e2e && npm ci --no-audit --no-fund)
if [ -z "${CHROMIUM_PATH:-}" ] && [ -d "${PLAYWRIGHT_BROWSERS_PATH:-/nonexistent}" ]; then
  shell=$(find "$PLAYWRIGHT_BROWSERS_PATH" -maxdepth 3 -type f -name chrome -path '*chromium_headless_shell*' 2>/dev/null | head -1)
  [ -n "$shell" ] || shell=$(find "$PLAYWRIGHT_BROWSERS_PATH" -maxdepth 3 -type f -name chrome 2>/dev/null | head -1)
  [ -n "$shell" ] && export CHROMIUM_PATH="$shell"
fi

enc() { node -p 'encodeURIComponent(process.argv[1])' "$1"; }
query="gfx=webgl2&quality=$QUALITY&knobs=$(enc "$KNOBS")"
for n in "${NAMES[@]}"; do
  echo "==> $n"
  node e2e/fxstrip.mjs page "$OUT/$n" --serve "$DIST" --hook fx --query "animstage=$n&$query" "${MORE[@]}"
done

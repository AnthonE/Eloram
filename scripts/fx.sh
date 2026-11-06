#!/usr/bin/env bash
# The fx stage (`docs/FX.md`, "The stage"): one effect at a time, fired in front of a dark card and
# a light card on a clock that steps 1/60 s, a frame a step, composed into a strip with its numbers
# and a diff against the last run (`el-client/src/stage.rs`, `e2e/fxstrip.mjs`).
#
#   scripts/fx.sh list [filter]                     # every effect the stage can fire
#   scripts/fx.sh knobs                             # every knob a recipe is retuned with
#   scripts/fx.sh orrin-q-impact liora-r-blast      # → e2e/artifacts/fx/<name>/{strip.png,numbers.json,diff.png}
#   scripts/fx.sh orrin-q-impact --knobs 'impact.star=1.5;size=0.8'   # retune, no rebuild
#   scripts/fx.sh orrin-q-impact --seed 7 --frames 15 --span 1.2      # another roll, more frames, a longer span
#   scripts/fx.sh orrin-q-impact --side enemy --size 300 --low        # as an enemy sees it, 300 damage, the low tier
#   scripts/fx.sh --no-build …                      # the page as it was last built
#   EL_FX_DIST=dir scripts/fx.sh --no-build …       # a copy of web/dist, while web/dist is rebuilt
#
# Builds the WebGL2 page (scripts/build-web.sh: wasm-bindgen-cli at the lock's version) and serves
# it itself: no game server. Needs Playwright's Chromium (e2e/: npm ci; the dev container keeps it
# in /opt/pw-browsers). Headless Chromium draws in software, a frame a second or two: a strip of
# eleven frames over a second's effect takes about two minutes.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=${EL_FX_OUT:-e2e/artifacts/fx}
# The page served (a copy of web/dist, to keep shooting while web/dist is rebuilt).
DIST=${EL_FX_DIST:-web/dist}
BUILD=1
NAMES=()
KNOBS=""
SEED=0
SIDE=own
SIZE=""
CARDS=""
QUALITY=high
MORE=()
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --knobs) KNOBS=$2; shift ;;
    --seed) SEED=$2; shift ;;
    --side) SIDE=$2; shift ;;
    --size) SIZE=$2; shift ;;
    --cards) CARDS=$2; shift ;;
    --low) QUALITY=low ;;
    --frames|--span|--steps|--timeout) MORE+=("$1" "$2"); shift ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) NAMES+=("$1") ;;
  esac
  shift
done
[ ${#NAMES[@]} -gt 0 ] || { sed -n '2,18p' "$0"; exit 2; }

case "${NAMES[0]}" in
  list) cargo run -q -p el-client-core --example fxstage -- list "${NAMES[@]:1}"; exit 0 ;;
  knobs) cargo run -q -p el-client-core --example fxstage -- knobs; exit 0 ;;
esac

if [ "$BUILD" = 1 ]; then
  ./scripts/build-web.sh webgl2
fi
[ -f "$DIST/webgl2/el_bg.wasm" ] || { echo "fx: no page in $DIST: run without --no-build" >&2; exit 1; }
if [ -z "${PLAYWRIGHT_BROWSERS_PATH:-}" ] && [ -d /opt/pw-browsers ]; then
  export PLAYWRIGHT_BROWSERS_PATH=/opt/pw-browsers
fi
[ -d e2e/node_modules ] || (cd e2e && npm ci --no-audit --no-fund)
# Headless Chromium picks its browser from Playwright's install; fxstrip takes an explicit one.
if [ -z "${CHROMIUM_PATH:-}" ] && [ -d "${PLAYWRIGHT_BROWSERS_PATH:-/nonexistent}" ]; then
  shell=$(find "$PLAYWRIGHT_BROWSERS_PATH" -maxdepth 3 -type f -name chrome -path '*chromium_headless_shell*' 2>/dev/null | head -1)
  [ -n "$shell" ] || shell=$(find "$PLAYWRIGHT_BROWSERS_PATH" -maxdepth 3 -type f -name chrome 2>/dev/null | head -1)
  [ -n "$shell" ] && export CHROMIUM_PATH="$shell"
fi

enc() { node -p 'encodeURIComponent(process.argv[1])' "$1"; }
query="gfx=webgl2&quality=$QUALITY&seed=$SEED&side=$SIDE&knobs=$(enc "$KNOBS")"
[ -n "$SIZE" ] && query="$query&size=$SIZE"
[ -n "$CARDS" ] && query="$query&cards=$(enc "$CARDS")"
for n in "${NAMES[@]}"; do
  echo "==> $n"
  node e2e/fxstrip.mjs page "$OUT/$n" --serve "$DIST" --hook fx --query "fxstage=$n&$query" "${MORE[@]}"
done

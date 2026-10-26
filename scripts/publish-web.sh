#!/usr/bin/env bash
# Publish the browser client to the Elo Pros origin: elopros.com/games/eloram/play/.
#
#   scripts/publish-web.sh --server game.example.com:4433 --http https://game.example.com/eloram/
#   scripts/publish-web.sh ... --no-build     # stage what web/dist already holds
#   scripts/publish-web.sh ... --dry-run      # everything but the upload and the flip
#   scripts/publish-web.sh ... --host morr    # the origin's ssh alias (default: morr; `local`: this box)
#   scripts/publish-web.sh ... --strand       # publish a page the live server will refuse
#
# `--server` is the game server's WebTransport host:port (its certificate CA-issued), `--http` its
# HTTP side (`status.json`, `records/`); both are required: the page is served from another site
# than the server, so it must name it.
#
# Env: EL_WEB_VARIANTS (default "webgl2 webgpu": both, the page picks; "webgl2" one), STATUS_URL (the live
# server's /status.json, for the wire check), OUT (the staging directory, default target/webpub).
#
# Before Colony's script (itself Gates' ci/publish_web.sh): every publish lands whole in its own
# directory, `<root>/<build>/`, and the page the world sees is `<root>/current`, a symlink flipped
# LAST: a half-uploaded build is never served, and a rollback is one `ln -sfn` to the previous
# directory. The build id is `<version>-g<sha>`, `-dirty` on a dirty tree. Every asset URL in the
# page is relative, so it works under any path prefix. The store's CSP allows no inline script:
# the page has none.
set -euo pipefail
cd "$(dirname "$0")/.."

HOST="morr"
ROOT="/data/apps/scry-data/eloram-web"
SERVER=""
HTTP=""
BUILD=1
DRY=0
STRAND=0
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --dry-run) DRY=1 ;;
    --host) HOST="$2"; shift ;;
    --root) ROOT="$2"; shift ;;
    --server) SERVER="$2"; shift ;;
    --http) HTTP="$2"; shift ;;
    --strand) STRAND=1 ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
  shift
done
if [ -z "$SERVER" ] || [ -z "$HTTP" ]; then
  echo "name the game server: --server host:port --http https://host/path/" >&2
  exit 2
fi
case "$HTTP" in */) ;; *) HTTP="$HTTP/" ;; esac

# Refuse a page the live server can't talk to, before the build: a page on another protocol
# version is refused at the handshake (VersionMismatch). Deploy the server first.
STATUS_URL="${STATUS_URL:-${HTTP}status.json}"
want="$(grep -oP 'pub const PROTOCOL_VERSION: u16 = \K[0-9]+' crates/el-proto/src/lib.rs)"
live="$(curl -s --max-time 15 "$STATUS_URL" | python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("proto", ""))
except Exception: print("")')"
if [ "$live" = "$want" ]; then
  echo "== wire ok: the live server speaks protocol $live, and so does this page"
elif [ "$STRAND" = 1 ]; then
  echo "== !! the live server speaks protocol ${live:-<unreported>} and this page $want: publishing anyway (--strand)"
else
  echo "the live server ($STATUS_URL) speaks protocol ${live:-<unreported>} and this page speaks $want:" >&2
  echo "every join would be refused. Deploy the server first, or pass --strand." >&2
  exit 1
fi

read -r -a variants <<< "${EL_WEB_VARIANTS:-webgl2 webgpu}"
if [ "$BUILD" = 1 ]; then
  ./scripts/build-web.sh "${variants[@]}"
fi

OUT="${OUT:-target/webpub}"
rm -rf "$OUT"
mkdir -p "$OUT"
cp -a web/dist/. "$OUT/"
for v in "${variants[@]}"; do
  for f in el.js el_bg.wasm; do
    [ -f "$OUT/$v/$f" ] || { echo "$OUT/$v/$f is missing: run scripts/build-web.sh ${variants[*]}" >&2; exit 1; }
    # gzip_static serves the .gz beside each module (build-web.sh writes .br and .gz).
    [ -f "$OUT/$v/$f.gz" ] || gzip -9 -k "$OUT/$v/$f"
  done
done
for f in index.html config.js boot.js loader.js wallet.js style.css favicon.svg; do
  [ -f "$OUT/$f" ] || { echo "$OUT/$f is missing: run scripts/build-web.sh" >&2; exit 1; }
done
# A variant left over from an older build would be picked (build.json names it) and would speak
# an older protocol: ship only what was asked for (and the art, which isn't a variant).
[ -d "$OUT/assets" ] || { echo "$OUT/assets is missing: run scripts/build-web.sh" >&2; exit 1; }
for d in "$OUT"/*/; do
  name="$(basename "$d")"
  [ "$name" = assets ] && continue
  printf '%s\n' "${variants[@]}" | grep -qx "$name" || { echo "== dropping stale $name/"; rm -rf "$d"; }
done
scripts/web-modules.sh "$OUT"

sha="$(git rev-parse --short=10 HEAD)"
version="$(grep -m1 -oP '^version = "\K[^"]+' Cargo.toml)"
ID="${version}-g${sha}"
[ -z "$(git status --porcelain --untracked-files=no)" ] || ID="${ID}-dirty-$(date -u +%Y%m%d%H%M%S)"

cat > "$OUT/config.js" <<JS
// Written by scripts/publish-web.sh (build ${ID}): this page is served from another site than the
// game server, so it names it. \`?server=host:port\` and \`?http=url\` still override it.
window.EL_SERVER = window.EL_SERVER || { wt: "${SERVER}", http: "${HTTP}" };
JS
cat > "$OUT/build.json" <<JSON
{"id": "${ID}", "commit": "$(git rev-parse HEAD)", "protocol": ${want}, "variants": "${variants[*]}", "server": "${SERVER}", "http": "${HTTP}", "built": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"}
JSON

DEST="$ROOT/$ID"
echo "== build $ID"
echo "   $(du -sh "$OUT" | cut -f1) staged, $(find "$OUT" -type f | wc -l) files; server $SERVER, http $HTTP"
echo "== to $HOST:$DEST  (current -> $ID after the upload)"
if [ "$DRY" = 1 ]; then
  echo "dry run: nothing uploaded, nothing flipped"
  exit 0
fi

if [ "$HOST" = local ]; then
  mkdir -p "$ROOT"
  rsync -a --info=stats1 --link-dest="../current" "$OUT/" "$DEST/"
  ln -sfn "$ID" "$ROOT/current.new" && mv -T "$ROOT/current.new" "$ROOT/current" && ls -l "$ROOT/current"
else
  # rsync creates the last path component and no more; the root is ours to make.
  ssh -o BatchMode=yes "$HOST" "mkdir -p '$ROOT'"
  # `--link-dest` is relative to the DESTINATION directory: `../current` is the build served now.
  rsync -a --info=stats1 --link-dest="../current" "$OUT/" "$HOST:$DEST/"
  # Flip last, atomically: a new symlink beside the old one, then one rename.
  ssh -o BatchMode=yes "$HOST" "ln -sfn '$ID' '$ROOT/current.new' && mv -T '$ROOT/current.new' '$ROOT/current' && ls -l '$ROOT/current'"
fi
echo "== live: https://elopros.com/games/eloram/play/"

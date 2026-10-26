#!/usr/bin/env bash
# Builds the browser client into web/dist/<variant>/ (variants: webgl2, webgpu).
#   scripts/build-web.sh            # both: the page picks WebGPU where the browser has it
#   scripts/build-web.sh webgl2     # one (the fast loop)
# Bevy picks its GPU API when it's compiled (the `webgpu` feature drops every WebGL workaround),
# so one module can't serve both: each is checked for the backend it was meant to bind, and
# web/dist/build.json names each one present by its hash (loader.js picks by it, and remembers a
# WebGPU module that failed by it).
# Env: EL_WEB_PROFILE (default wasm-release), EL_WEB_OPT=1 to run wasm-opt when available;
# CARGO_TARGET_DIR is followed.
set -euo pipefail
cd "$(dirname "$0")/.."

variants=("$@")
[ ${#variants[@]} -eq 0 ] && variants=(webgl2 webgpu)
profile="${EL_WEB_PROFILE:-wasm-release}"

# wasm-bindgen's CLI must match the crate version exactly.
crate_ver=$(awk '/^name = "wasm-bindgen"$/{getline; print $3}' Cargo.lock | tr -d '"' | head -1)
cli_ver=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')
if [ -z "$cli_ver" ]; then
  echo "wasm-bindgen CLI not found: cargo install wasm-bindgen-cli --version ${crate_ver} --locked" >&2
  exit 1
fi
if [ "$crate_ver" != "$cli_ver" ]; then
  echo "wasm-bindgen CLI ${cli_ver} != crate ${crate_ver}" >&2
  exit 1
fi

mkdir -p web/dist
cp web/index.html web/config.js web/boot.js web/loader.js web/wallet.js web/style.css web/favicon.svg web/dist/
# The art (docs/ART.md): the ground's textures, the brush, the models. JPEG and PNG are compressed
# already; the models' geometry isn't, so they get precompressed copies like the wasm's.
rm -rf web/dist/assets
cp -r web/assets web/dist/assets
node -e '
  const fs = require("fs"), z = require("zlib"), path = require("path");
  for (const f of fs.readdirSync(process.argv[1]).filter((f) => f.endsWith(".glb"))) {
    const p = path.join(process.argv[1], f), raw = fs.readFileSync(p);
    fs.writeFileSync(p + ".br", z.brotliCompressSync(raw, { params: { [z.constants.BROTLI_PARAM_QUALITY]: 9 } }));
    fs.writeFileSync(p + ".gz", z.gzipSync(raw, { level: 9 }));
  }' web/dist/assets/models

for v in "${variants[@]}"; do
  feats=()
  case "$v" in
    webgl2) ;;
    webgpu) feats=(--features webgpu) ;;
    *) echo "unknown variant $v" >&2; exit 1 ;;
  esac
  echo "==> building $v ($profile)"
  cargo build -p el-client --target wasm32-unknown-unknown --profile "$profile" "${feats[@]}"
  out="web/dist/$v"
  rm -rf "$out"
  wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name el \
    "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/$profile/el-client.wasm"
  if [ "${EL_WEB_OPT:-0}" = "1" ] && command -v wasm-opt >/dev/null; then
    echo "==> wasm-opt -Oz $v"
    wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int -o "$out/el_bg.wasm" "$out/el_bg.wasm"
  fi
  # Precompressed copies; the server serves them when the browser accepts br/gzip.
  node -e '
    const fs = require("fs"), z = require("zlib");
    for (const f of process.argv.slice(1)) {
      const raw = fs.readFileSync(f);
      fs.writeFileSync(f + ".br", z.brotliCompressSync(raw, { params: { [z.constants.BROTLI_PARAM_QUALITY]: 9 } }));
      fs.writeFileSync(f + ".gz", z.gzipSync(raw, { level: 9 }));
    }' "$out/el_bg.wasm" "$out/el.js"
  # Proof of which backend was bound: only wgpu's WebGPU backend asks the browser for an adapter.
  # A WebGPU module without it (or a WebGL2 one with it, a feature unified in from somewhere)
  # would load and draw nothing rather than fail here.
  if grep -qa 'requestAdapter' "$out/el_bg.wasm"; then got=webgpu; else got=webgl2; fi
  [ "$got" = "$v" ] || { echo "$v: bound a $got module" >&2; exit 1; }
  raw=$(stat -c %s "$out/el_bg.wasm"); br=$(stat -c %s "$out/el_bg.wasm.br")
  echo "==> $v: wasm $((raw / 1024 / 1024)) MiB raw, $((br / 1024 / 1024)) MiB brotli"
done
scripts/web-modules.sh web/dist

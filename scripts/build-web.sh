#!/usr/bin/env bash
# Builds the browser client into web/dist/<variant>/ (variants: webgl2, webgpu).
#   scripts/build-web.sh            # webgl2 only (fast path)
#   scripts/build-web.sh webgl2 webgpu
# Env: EL_WEB_PROFILE (default wasm-release), EL_WEB_OPT=1 to run wasm-opt when available;
# CARGO_TARGET_DIR is followed.
set -euo pipefail
cd "$(dirname "$0")/.."

variants=("$@")
[ ${#variants[@]} -eq 0 ] && variants=(webgl2)
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
  raw=$(stat -c %s "$out/el_bg.wasm"); br=$(stat -c %s "$out/el_bg.wasm.br")
  echo "==> $v: wasm $((raw / 1024 / 1024)) MiB raw, $((br / 1024 / 1024)) MiB brotli"
done

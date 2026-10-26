#!/usr/bin/env bash
# Writes <dir>/build.json: each game module present under <dir> (webgl2/, webgpu/) by the hash of
# its glue and wasm together. loader.js picks a module by it, and remembers a WebGPU module that
# failed by it (so the next build is tried afresh).
#   scripts/web-modules.sh web/dist
set -euo pipefail
dir="$1"
entries=()
for v in webgl2 webgpu; do
  [ -f "$dir/$v/el_bg.wasm" ] || continue
  sha=$(cat "$dir/$v/el.js" "$dir/$v/el_bg.wasm" | sha256sum | cut -c1-16)
  entries+=("\"$v\": \"$sha\"")
done
(IFS=,; echo "{${entries[*]}}") > "$dir/build.json"
echo "==> $dir/build.json: $(cat "$dir/build.json")"

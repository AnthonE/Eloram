#!/usr/bin/env bash
# The models: Meshy's .glb downloads (art/meshy.json names each task) made fit to ship. Each is
# stood on the origin (its base at y = 0, on its node: the client stands the mesh itself, as it
# loads the primitive alone), welded, simplified (a top-down camera at this size doesn't see the
# rest: a third of the vertices), given tangents for its normal map, its textures cut to size and
# turned to JPEG, and written to web/assets/models/<name>.glb.
#   art/tools/models.sh <dir of downloaded .glb> [gltf-transform binary]
# The downloads don't go in git; `art/tools/meshy.py fetch <dir>` gets them again while Meshy
# keeps them.
set -euo pipefail
src="$1"
gt="${2:-gltf-transform}"
out="$(cd "$(dirname "$0")/../.." && pwd)/web/assets/models"
mkdir -p "$out"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# name, texture size (the camera looks down from 23 m: none shows more than 512).
while read -r name size; do
  [ -z "$name" ] && continue
  in="$src/$name.glb"
  [ -f "$in" ] || { echo "missing $in" >&2; exit 1; }
  "$gt" center "$in" "$tmp/a.glb" --pivot below >/dev/null
  "$gt" weld "$tmp/a.glb" "$tmp/w.glb" >/dev/null
  "$gt" simplify "$tmp/w.glb" "$tmp/b.glb" --ratio 0.35 --error 0.004 >/dev/null
  "$gt" tangents "$tmp/b.glb" "$tmp/c.glb" >/dev/null
  "$gt" resize "$tmp/c.glb" "$tmp/d.glb" --width "$size" --height "$size" >/dev/null
  "$gt" jpeg "$tmp/d.glb" "$tmp/e.glb" --quality 85 >/dev/null
  "$gt" prune "$tmp/e.glb" "$out/$name.glb" >/dev/null
  echo "$name $(( $(stat -c %s "$out/$name.glb") / 1024 )) KiB"
done <<'EOF'
turret 512
inhibitor 512
nexus 512
fountain 512
brazier 512
rock_a 512
rock_b 512
pillar 512
statue 512
spire 512
arch 512
banner 512
EOF

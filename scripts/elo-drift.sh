#!/usr/bin/env bash
# Checks the copies Eloram keeps of the Elo Pros platform's files against the platform's own:
# the launcher SDK (crates/el-launcher/src/elo_overlay.rs against sdk/SHA256SUMS) and the test
# vectors el-auth checks itself with (sdk/testdata/). A difference means: fix it upstream, copy it
# back (the copies are never edited here).
#   scripts/elo-drift.sh /path/to/scry-forge
set -euo pipefail
cd "$(dirname "$0")/.."
platform="${1:?name the platform checkout, scry-forge}"
fail=0
want=$(awk '$2 ~ /elo_overlay.rs$/ {print $1}' "$platform/sdk/SHA256SUMS")
have=$(sha256sum crates/el-launcher/src/elo_overlay.rs | cut -d' ' -f1)
if [ "$want" = "$have" ]; then echo "ok    elo_overlay.rs ($have)"; else echo "DRIFT elo_overlay.rs: ours $have, the platform's $want"; fail=1; fi
for f in crates/el-auth/tests/testdata/*.json; do
  up="$platform/sdk/testdata/$(basename "$f")"
  if [ ! -f "$up" ]; then echo "gone  $(basename "$f") (no longer in the platform's sdk/testdata)"; fail=1
  elif cmp -s "$f" "$up"; then echo "ok    $(basename "$f")"
  else echo "DRIFT $(basename "$f")"; fail=1; fi
done
exit $fail

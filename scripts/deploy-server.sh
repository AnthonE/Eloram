#!/usr/bin/env bash
# Release the server on the game box (ops/README.md), after oneshot's ci/deploy_server.sh. Run it
# on the box, in the checkout:
#
#   scripts/deploy-server.sh          # a dry run: what it would do
#   scripts/deploy-server.sh --go     # build, stage, install the unit, the hook and the nginx file, restart
#
# One slot: a restart takes the live matches up again from their records and checkpoints
# (`el-server/src/recover.rs`), and their players rejoin with their tokens, so there is no second
# slot to switch to and nothing to drain. The binary and the ops files are staged in
# /home/master/eloram-deploy; the data is /home/master/eloram-data; the certificate copies are
# /home/master/eloram-certs, made by the certbot hook.
set -uo pipefail
cd "$(dirname "$0")/.."

DEPLOY=${DEPLOY:-/home/master/eloram-deploy}
DATA=${DATA:-/home/master/eloram-data}
HOOK=/etc/letsencrypt/renewal-hooks/deploy/copy-eloram-cert.sh
GO=0
for a in "$@"; do
  case "$a" in
    --go) GO=1 ;;
    *) echo "unknown flag: $a" >&2; exit 2 ;;
  esac
done
run() { if [ "$GO" = 1 ]; then echo "+ $*"; "$@" || exit 1; else echo "  would run: $*"; fi; }
differs() { ! sudo -n cmp -s "$1" "$2" 2>/dev/null; }

echo "== build"
run cargo build --release -p el-server
bin="${CARGO_TARGET_DIR:-target}/release/el-server"
[ "$GO" = 0 ] || [ -x "$bin" ] || { echo "no $bin" >&2; exit 1; }

echo "== stage into $DEPLOY"
run install -d -o master -g master -m 755 "$DEPLOY" "$DEPLOY/no-web-here" "$DATA"
run install -o master -g master -m 755 "$bin" "$DEPLOY/el-server.new"
run mv -f "$DEPLOY/el-server.new" "$DEPLOY/el-server"
run install -o master -g master -m 644 ops/game-elopros-eloram.nginx.conf "$DEPLOY/game-elopros-eloram.nginx.conf"

echo "== the unit, the hook"
if differs ops/eloram.service /etc/systemd/system/eloram.service; then
  run sudo install -m 644 ops/eloram.service /etc/systemd/system/eloram.service
  run sudo systemctl daemon-reload
fi
if differs ops/certbot-deploy-hook.sh "$HOOK"; then
  run sudo install -m 755 ops/certbot-deploy-hook.sh "$HOOK"
fi
if [ ! -f /home/master/eloram-certs/fullchain.pem ]; then
  echo "== no certificate copies yet: the hook makes them from the box's lineage"
  run sudo "$HOOK"
fi

echo "== restart"
run sudo systemctl enable --now eloram.service
run sudo systemctl restart eloram.service
if [ "$GO" = 1 ]; then
  for _ in $(seq 1 50); do
    if curl -sf http://127.0.0.1:8460/status.json >/dev/null; then
      echo "== up: $(curl -s http://127.0.0.1:8460/status.json | head -c 200)"
      exit 0
    fi
    sleep 0.2
  done
  echo "!! the server didn't answer on 8460: journalctl -u eloram -n 50" >&2
  exit 1
fi
echo "== (dry run) the nginx include, if it isn't in the game.elopros.com block yet:"
echo "   include $DEPLOY/game-elopros-eloram.nginx.conf;   then sudo nginx -t && sudo systemctl reload nginx"

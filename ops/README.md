# Hosting Eloram

The server on the game box (`game.elopros.com`, the Hetzner VM Gates and oneshot run on), its
page on elopros.com. The way oneshot is hosted, with one difference the record makes: there is
**one slot, and a restart loses nothing**. Each match's record, checkpoint and lobby file stay on
disk, the next server on the same `--records` takes them up from the last verified checkpoint, and
the players come back to their seats with their tokens (`docs/NETCODE.md`, "The record";
`el-server/src/recover.rs`, measured in `tests/recover.rs`: four matches taken up in under half a
second, about 120 bytes of record lost each). So a deploy is a restart, and nobody needs a second
slot to drain into.

A renewal reloads the certificate instead of restarting (SIGHUP; nobody is dropped), and a running
server remakes a self-signed one before browsers' fourteen days are up (`net::endpoint::rotate`).

## The pieces

| What | Where | How |
|---|---|---|
| The game (WebTransport, UDP) | `game.elopros.com:61260` | `eloram.service`, user `master` |
| `status.json`, `cert-hash`, `online.json`, `records.json`, `records/<id>` | `https://game.elopros.com/eloram/…` | the box's nginx on 443 → `127.0.0.1:8460` (`game-elopros-eloram.nginx.conf`) |
| The page | `https://elopros.com/games/eloram/play/` | the platform's nginx on `morr` (`scripts/publish-web.sh --server game.elopros.com:61260 --http https://game.elopros.com/eloram/`) |
| Certificate | `/home/master/eloram-certs/{fullchain,privkey}.pem` | Let's Encrypt for `game.elopros.com` (the box's lineage), copied by `certbot-deploy-hook.sh` |
| Binary, ops files | `/home/master/eloram-deploy/` | `scripts/deploy-server.sh` |
| Data | `/home/master/eloram-data/` | records (`{id}.elr`, `.part`, `.ck`), the lobby's file, `tokens.json` (the resume tokens' hashes), `rerolls.json` |

No Docker; no `cargo` at run time. The unit runs the staged binary, never the checkout.

## The first time, on the box

1. **The port.** UDP 61260 in **both** ufw and the Hetzner Cloud Firewall (the cloud firewall
   passes only 49152–65535; ufw alone does nothing): `sudo ufw allow 61260/udp`, then the same
   rule in the Hetzner console. Taken already: Gates 61234, Before Colony 61240, oneshot
   61250–61251, Fireplace 61290–61297, kit rooms 61300–61315. The port is the unit's and the
   nginx file's and the page's (`--server`): one decision in three places.
2. **The certificate.** The box's `game.elopros.com` lineage already exists (Gates').
   `scripts/deploy-server.sh --go` installs the hook and runs it once to make the copies.
3. **nginx**: `include /home/master/eloram-deploy/game-elopros-eloram.nginx.conf;` inside the
   `game.elopros.com` 443 server block, then `sudo nginx -t && sudo systemctl reload nginx`.
4. **UDP buffers.** The server asks for 8 MiB each way and logs what it got if the kernel
   clamped it: `sudo sysctl -w net.core.rmem_max=16777216 net.core.wmem_max=16777216` (and the
   same in `/etc/sysctl.d/90-eloram.conf`; oneshot asks for the same, so the box may have it).
5. `scripts/deploy-server.sh --go`, then from a workstation `scripts/publish-web.sh --server
   game.elopros.com:61260 --http https://game.elopros.com/eloram/` (it refuses a page the live
   server can't talk to).
6. On Elo Pros: the listing's `play_url`, the nginx block for the page, and the playtime poller
   on `online.json` (`docs/ROADMAP.md`, phase 5).

Not taken from Gates: its `ops/gates-udp443.nft`, which forwards UDP 443 to the shard's port so
a network that passes only web ports can still reach it. Only one thing can own UDP 443 on an
address, and on this box it is Gates. Eloram could have it on an address of its own; until then
a player behind such a network can't play, and the page says only that the server didn't
answer.

## Day to day

- **Release the server:** `scripts/deploy-server.sh` (a dry run), then `--go`. The restart takes
  the live matches up again; `journalctl -u eloram -f` says which, and `/status.json`'s `game`
  says what's running.
- **Release the page:** `scripts/publish-web.sh …`. It refuses a page whose protocol the live
  server doesn't speak: deploy the server first, then the page. Rollback is one `ln -sfn` on
  `morr`.
- **Renewal:** certbot runs the hook; it copies and `systemctl reload`s the server (SIGHUP). Nobody
  is dropped. `journalctl -u eloram | grep certificate`.
- **Logs:** `journalctl -u eloram -f`: a `status:` line every ten seconds that something moved
  (players, matches, datagrams, and what should stay at zero: refusals, bad datagrams, shard
  overruns, output drops, records astray, referee divergences, hot-path allocations), the
  certificate's reloads, each match taken up after a restart.
- **The unit changed** (a new flag)? `scripts/deploy-server.sh --go` installs it and reloads
  systemd before the restart.
- **Health:** `curl -s https://game.elopros.com/eloram/status.json` (the wire's protocol, the
  players, the shards' ticks and traffic, the referee's hashes and checkpoints: a `divergences`
  above zero is a bug in the sim, `docs/NETCODE.md`).

## Backups

`/home/master/eloram-data` is the record: every finished match's `.elr`, re-stepped by `el-replay
verify`. Back up the records; `tokens.json` and `rerolls.json` are the players' resume tokens'
hashes and banked rerolls, worth keeping; the certificate copies are remade by the hook.

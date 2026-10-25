# Roadmap

How Eloram gets from an empty repo to ARAM: Mayhem in a browser tab. Each phase ends with
`scripts/ci.sh` green and is pushed before the next starts. `NETCODE.md` is what the netcode phases
build; `DESIGN.md` is what the sim phases build.

## 0. The research, written
`NETCODE.md` (League's netcode from Riot's posts, our three netcodes beside it, what we build and
what we turned down), `DESIGN.md` (the mode at League's numbers, in our names), this file.

## 1. The workspace and the copied core
Before Colony's workspace, toolchain and lints; its counting allocator, bit writer, acks,
quantizers, storage, maths, jitter buffer, metrics, WebTransport endpoint and admission, and a Bevy
scene that says hello to the server from a browser.

## 2. The match, headless
The Causeway's navgrid, paths, motion and vision; units; the effect system; five champions, a
dozen augments, a few items; minions, turrets, the economy and the augment screens; a match from
first wave to a fallen nexus between bots. Golden hashes native and wasm over a whole match, no
allocation in a tick, a bench.

## 3. The netcode, end to end
Commands and orders, team-filtered deltas against acked baselines, missiles as events; the
client's clock, prediction and interpolation (`el-client-core`); the server's shards, egress
threads, sessions and lobby (a queue, bots in the empty seats after `--fill-after`, champions dealt
at random); agents over WebTransport (`el-bot`, `el-swarm`). A lossy-link test (prediction error,
orders applied once), the vision-leak oracle, no allocation in a shard tick, matches a core.

Measured when it landed:
- Over 100 ms RTT, ±20 ms jitter and 5% loss (`el-match/tests/netcode.rs`): walking predicted
  exactly (p99 0.00 units over about 3,400 corrections a client in two minutes), and every order
  applied once. Down 10 to 12 KB/s a client, the largest datagram 743 B; up under 2 KB/s.
- The leak oracle (`vision_leak.rs`): 0 leaks over 628,798 records and 16,110 events in five
  minutes of play; it catches an unseen unit's record, or an unseen champion's cast, the tick
  either is let through.
- A shard of 64 matches, 640 seats replicated (`cargo bench -p el-match --bench shard`), minutes
  2 to 4: a shard tick 13.9 ms at p50 and 26 ms at p99 of the 33 ms budget, a match's tick 218 µs
  at p50: 82 matches a core at p99.
- 25 agents over WebTransport on loopback (`el-swarm`): 30.1 snapshots a second each, 7.5 KB/s
  down, no stale or bad datagrams, all 2,751 orders acked, no walking correction of a unit or more,
  `hot_path_allocations` 0.

Left for later phases: the record (Phase 5, with the referee), the champion reroll and bench
(Phase 4, with the page that shows them), the agents' own team-view bots in the empty seats
(the server's bots read the match, held to what their team sees by `el_sim::bot`).

## 4. The client, playable
The Bevy client: the camera, orders by mouse and keys, champions, minions, missiles drawn on the
predicted clock, fog, the HUD, the shop, the augment cards. You and nine bots in a browser.

## 5. Mayhem, the record, spectating, Elo Pros
The augment set across four screens, Combo Breaker, headliners, more champions. The referee,
replays and the 60 s spectator feed from the record. Wallet sign-in, `/status.json` and
`/online.json`, publishing to the store.

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

Then the wire again (protocol 3, `NETCODE.md`, "The wire"): the own champion as fields against the
state the client acked, a unit's record once and once more and again only when an ack names it
lost, baselines however old, input packets a tick rather than a frame; and what a team says, pings
and the vote to surrender, as orders with team-only events. Measured against the commit before:
- Down a client over 100 ms and 5% loss, 11.6 to 3.8 KB/s (the own champion 212 B a datagram to
  18), up 1.9 to 0.8; walking still predicted exactly, every order applied once, and what a client
  takes its team's view to be is the server's for 99.9% of the units it sees (a new check in
  `netcode.rs`, which also rides out a 3 s outage).
- The shard bench (64 matches): 9.7 to 3.2 KB/s a seat, a match's tick 136 to 127 µs at p50,
  149 to 158 matches a core at p99.
- 25 agents over WebTransport (`el-swarm`, 30 s): 1.0 KB/s down each (7.5 when this phase landed),
  no stale or bad datagrams, all 2,749 orders acked, `hot_path_allocations` 0.

## 4. The client, playable
The Bevy client: the camera, orders by mouse and keys, champions, minions, missiles drawn on the
predicted clock, fog, the HUD, the shop, the augment cards. You and nine bots in a browser.

What landed: the queue from the page (`?queue=0` stays in the lobby), League's default keys
(`DESIGN.md`, "Controls"), a camera locked on the champion or free, the Causeway drawn from the
map's own shapes, every unit slot an entity with its health bar, missiles at the own clock, area
flashes, relics, fog from the same sight windows the server uses (`el_client_core::fog`, checked
against the server's grids), and the HUD in Bevy UI: abilities with their cooldowns, health, mana,
experience, gold, items, the kill feed, a minimap, the augment cards, the shop, the scoreboard.
`scripts/dev.sh` serves it with the bots filling in; `scripts/e2e.sh match` plays one in headless
Chromium (queue, augment, shop, a skill, Blink, a walk with no correction at all, the enemy seen
only once the waves meet).

Not yet: champion art (Phase 5 gave each role a figure built from primitives, in its champion's
colour: `el-client/src/models.rs`), WASD bound on the page (the champion select, five more
champions and cast indicators came with Phase 5). Not measured: the frame
rate on a real GPU (headless Chromium here draws in software, about a frame a second at 960×540).

## 5. Mayhem, the record, spectating, Elo Pros
The augment set across four screens, Combo Breaker, headliners, more champions. The referee,
replays and the 60 s spectator feed from the record. Wallet sign-in, `/status.json` and
`/online.json`, publishing to the store.

The record landed first: written by the shard thread without allocating, kept by a recorder thread,
served at `/records/{id}` a minute behind, re-stepped by `el-replay verify` and by the page that
watches it (`scripts/e2e.sh watch`). A match reset in place now starts every slot's generation from
0, as a new one does, so its record replays on a fresh match. Still to come of it: the live
referee thread, checkpoints, crash recovery.

Then Elo Pros: wallet sign-in (`el-auth`, Before Colony's, signing the launcher's text for
`eloram`) from the page (`?signin=1`) and from agents with a key, resume tokens, `/online.json`
for the store's playtime, `proto` on `/status.json`, and `scripts/publish-web.sh` (the build onto
`elopros.com/games/eloram/play/`, `current` flipped last). Still the operator's: where the game
server runs (`--server`, `--http`, `--siwe-domain`), the store's route to the page, and the
listing.

Then Mayhem's content: twenty champions over six roles, dealt ten different a match; 41 augments,
about a third a tier, with a trigger on a cast beside the others; a headliner a match, drawn from
the seed and shown as it starts. Names that were Riot's went (items, two runes, an ability, the
summoner spells: D is Blink, F is Tag). Measured: the golden bot match (seed 11) ends at 14:00 and
seed 7's at 15:54; a match's tick 77 µs at p50 and 357 µs at p99 (`match_tick`, 26,813 ticks), as
it was with five champions (73 and 389). That p99 is over the 150 µs this plan set, as it already
was; the shard bench's 82 matches a core at p99 is the number that holds. Every augment on every
champion ticks without allocating (`no_alloc.rs`).

And ARAM's champion select, in the lobby (`el-server/src/lobby.rs`): 20 s (`--select-secs`) in which
a reroll sends your champion to your team's bench and deals you one no one holds (two a seat), a
teammate's benched champion can be taken, and a team sees only its own; the match starts on time
or once everyone seated is ready, with the picks the select ended on in the record's header.
Agents are ready at once. `scripts/e2e.sh match` rerolls, takes the dealt champion back off the
bench, and plays it.

Then the links, made hard to break (`NETCODE.md`, "Many matches a process"):
- The egress ring could misframe a seat for good: the shard committed a length and its snapshot
  separately, and the egress thread could read in between. Every snapshot that seat got after
  that was garbage, and the page froze. Each frame now goes in one commit (`write_packet`), and
  the reader takes a frame only when all of it is there; a test races a writer thread against
  the reader.
- A client that stops reading its control stream can no longer stall its session; lobby
  requests are rate-limited.
- A dropped link or a reload rejoins the same seat (an online test over WebTransport, and
  `scripts/e2e.sh match` in the browser, where the redial is back in about a second and a half).
- The page's frame: the fog is uploaded only when it changes; units, missiles, HUD bars and
  minimap dots are written only when they move (a write marks a component changed, which costs
  propagation, extraction and UI layout); flashes fade in steps; health bars are drawn opaque;
  `window.__el` is converted to JavaScript only when it is published.

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

Left then, and done since: champion art (Phase 5 gave each role a figure built from primitives,
`el-client/src/models.rs`, still the stand-in while a model loads; the models came in Phase 7), WASD
bound on the page (`?keys=wasd`, League's layout), the champion select, five more champions and
cast indicators (Phase 5). Still not measured: the frame rate on a real GPU (headless Chromium here
draws in software, about a frame a second at 960×540).

## 5. Mayhem, the record, spectating, Elo Pros
The augment set across four screens, Combo Breaker, headliners, more champions. The referee,
replays and the 60 s spectator feed from the record. Wallet sign-in, `/status.json` and
`/online.json`, publishing to the store.

The record landed first: written by the shard thread without allocating, kept by a recorder thread,
served at `/records/{id}` a minute behind, re-stepped by `el-replay verify` and by the page that
watches it (`scripts/e2e.sh watch`). A match reset in place now starts every slot's generation from
0, as a new one does, so its record replays on a fresh match. The live referee, checkpoints and
crash recovery came after the effects (Phase 6, "Then the record, finished").

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

## 6. Effects and sound
`FX.md` is the plan: what Riot's posts say makes League's effects and sound read, the wire they
need, particles as closed forms, Web Audio, a practice mode, a look and a sound for every ability,
and the mix. Before champion animation, which will be timed to the same windups.

The wire landed first (protocol 4): zones told as closed forms (`ZoneSpawn`, `ZoneEnd`), so a
blast that lands late can be telegraphed to whoever sees its ground; damage with its kind and
source, heals and shields with theirs, areas with theirs; flags for a buff and a haste. On the way,
two faults: `Source` codes gave champions 16 to 19 the codes of 0 to 3, and a field that follows
its owner gave away an owner hidden in brush by its pulse. The leak oracle checks the new events
(0 leaks over 14,734 events, 62 zones; the old rule for a pulse leaks twice in the same match), and
down a client is 3.7 to 3.8 KB/s, as before.

Then the runtime and the sound (`FX.md`, "What landed"): a look and a sound for every ability,
derived from what it does and its champion's family; the director's cues, with the own
champion's from the prediction; bursts as closed forms in the shader; missiles' trails,
telegraphs, fields and statuses; Web Audio with a priority ladder, ducking and stereo from where
a sound is on the screen; 226 takes made from Kenney's CC0 packs and synthesis; the announcer
and music wired for their files. In a browser match: 2,632 cues in three minutes, at most 12
sounds at once, none dropped, the walk still exact.

Then the record, finished (`NETCODE.md`, "The record"), checked against Riot's posts on determinism
and Chronobreak first: a crash recovered by re-stepping the record (Riot tried save-states of memory
and dropped them), a checkpoint only a cache of the record, and a divergence named by comparing
state, not by a checksum, since "only the first divergence really matters".
- A whole match as bytes (`el_sim::snapshot`): 156 KB (5.6 KB gzipped, as `/records/{id}/checkpoint`
  serves it), written in about 240 µs and read in 300, the same native and wasm (a golden digest on
  both), read back and stepped for minutes alike tick for tick, written and read without allocating.
  A field added to the match doesn't compile until the checkpoint has it.
- The shard writes each match's checkpoint once a minute; the referee on each recorder thread
  re-steps every live record and keeps a checkpoint only once it's the same bytes, else names the
  unit and the field. The shard bench (64 matches): p99 23.6 ms with checkpoints, 23.9 without, 90
  and 89 matches a core. The referee: about 83 µs a tick re-stepped, a match's sim again (40 agents
  in four matches: 18,012 ticks in 1.5 s, 600 hashes and 11 checkpoints, none astray).
- Spectators start from the last verified checkpoint a minute behind (`/records/{id}/checkpoint`),
  not the record's first tick; `el-replay verify` resumes each `{id}.ck` to the record's end.
- A server killed outright, or stopped, leaves each live match's record part, checkpoint and lobby
  file; the next one on the same `--records` takes them up from the last hash and the players rejoin
  with their tokens: four matches two and a half minutes in, killed with SIGKILL, were taken up by
  the next server in under half a second, 56 to 83 ms each, about 120 bytes of record lost each
  (`el-server/tests/recover.rs`, and `scripts/e2e.sh recover` under the page). The self-signed
  certificate and the resume tokens' hashes are kept beside them, and the page asks for the
  certificate's hash again on every redial. Rejoin and resume tokens are kept only as their
  keccak-256.
- A match that panics stops alone, no contest, its shard's others going on; a shard that stops
  ticking is taken as gone.

Then the effects for a match watched (`FX.md`, "What landed"): a record stepped in the page tells
what each tick did as a seat is told it, nothing in fog (`el_client_core::spectator`), and the
director, the missiles, the telegraphs and the champions' casts read it as they read a client.
A watched match had drawn no missiles and played no sound before.

## 7. The champions, drawn
`CHAMPIONS.md` is the plan: what Riot's posts say makes a champion read (its silhouette and
primary feature, animation as the game's tell timed to it, the hitbox as where the champion is),
a model each with what it holds, clips timed to the windups, the body never leaving its place.

What landed:
- Twenty champions made with Meshy (text-to-3D in an A-pose, textured, rigged), twenty-three props
  for what they hold and carry, and nineteen clips from Meshy's library made once on one rig and
  retargeted onto the others (checked against Meshy's own retarget: the same pose frame for frame).
  `art/tools/champions.mjs` puts each together: its kit's clips cut to their windows, its props on
  its bones, simplified to about six thousand triangles, packed small.
- `el_client_core::pose`: each champion's kit, and the clip, time and fade a frame from what the
  client knows; attacks strike as the windup ends, casts release as their ability does.
- `el-client/src/champions.rs`: the models fetched as the match (or champion select) deals them,
  animated in their units' places; a body lies where it fell; a hit flashes.

Measured: twenty models of 494 to 656 KiB (6.6 MiB brotli for all twenty, about 3.3 for a match's
ten), 7,000 to 9,800 triangles each with its props, nine to eleven clips. In a browser match
(`scripts/e2e.sh match`) eleven models in (the team's own and a benched one in champion select),
the own champion standing, then running, attacking and casting as the pose says, the walk still
predicted exactly, a fallen champion lying where it fell; in a replay (`watch`) all ten run.
Headless Chromium draws 0.69 to 0.74 frames a second in SwiftShader (0.77 before). Not measured: a
real GPU.

Then the minions (`CHAMPIONS.md`, "The minions"): the last primitives on the lane became models,
two rigged bodies (a foot soldier, an acolyte) and a cannon cart from Meshy, the foot soldier
again with a great hammer for the super minion; the champions' clips retargeted onto them, timed
to their windups as the champions' are; one model for both teams, Dusk's colours its own texture
and the band of what each holds its team's; a minion that falls lies a moment and sinks. On the
way, Brun's hammer turned the right way round. Measured: four models of 115 to 301 KiB, 589 KiB
as served with Dusk's textures, 2,000 to 3,000 triangles each; in a browser match twenty minions
drawn at once, walking and swinging, the walk still exact, 0.69 frames a second in SwiftShader as
before; 55 of Meshy's last 56 credits.

Then the rest of what phases 6 and 7 left: a rim of each champion's team light where it turns from
the camera (the outline Riot inks, lit instead); a minion's flash when hit, and the cart's wheels
cut out of its mesh to turn as it rolls; the kill feed of a match watched. And practice
(`FX.md`, step 4): `el-server --practice` and `?practice=1`, one seat as any champion with every
rank and nothing on cooldown, nine dummies, no minions, its record saying so; and `scripts/e2e.sh
fx`, every ability of every champion cast there, drawn and heard, with a contact sheet a champion.
Measured: the golden hashes held (a checkpoint is a byte longer, `snapshot::VERSION` 2); in a
browser match 149 minions flashed and the walk was still exact, 204 frames over the suite's main
test against 208 before; in practice all eighty of the champions' abilities drawn and heard, at
most 10 sounds at once. A frame every two seconds in SwiftShader still stalls the page past the
server's 20 s of quiet at each switch of practice champion, and the page dials again.

Then the switch the fx suite now and then lost (`FX.md`, step 4): not a lost message but the last
match's snapshots, queued behind a slow frame and fed to the new match's client, which took its
clock (and at times its champion) from them and sent every command far into the server's future.
Each seating now has a stream of its own, named by the lobby in its `Match`, and the client takes
no other; a snapshot that doesn't decode moves nothing. An agent that stalls two seconds before
each switch (`el-bot/tests/online.rs`) dropped 60 of the last match's snapshots a switch and was in
the new match with the champion it asked for; without the fix the first switch went on as the old
champion, the server standing in for every command. The golden hashes don't move (nothing in the
sim changed); the wire has a field more in the `Match` message and nothing more in a snapshot,
and an older page or server simply takes any stream, as before.

Then the camera and the HUD, as MOBA players know them (`DESIGN.md`, "Controls" and "The HUD"): the
camera across the lane diagonally as ARAM's is, 56° down through a 40° lens, free to start and
locked with Y; the HUD in one panel at the bottom's middle, with abilities' painted icons and their
cooldowns swept clockwise, the champion's live portrait and stats, item icons, the allies' live
portraits down the left with their ultimates' orbs, a target frame, tooltips read from the sim's
tables, health bars ticked by the hundred, the shop in the middle with a Recommended row, the
death screen's grey and recap, and a square minimap turned as the camera turns.

Then the draw calls (`ART.md`, "What landed"; `scripts/e2e.sh draws` counts WebGL2's a frame, by
shader and mesh, which doesn't depend on the GPU): 189 to 217 a frame on the low tier and 289 to
316 on the high, to 92 to 108 and 146 to 159, about half. The bursts' pool drawn only while a burst
lasts, the plinths one mesh, health bars' backs opaque, and the HUD's text, slots and thin bars
drawn in fewer calls with the same look. What's left in the HUD is mostly its icons, portraits and
panels' gradients.

## 8. What the design said and the game didn't have, and what's left

First the rules `DESIGN.md` described that the sim didn't keep (`CONTENT_VERSION` 4,
`snapshot::VERSION` 3, protocol unchanged but for an order):
- **The trinket**: treats (Hearthbread), one to start and one more for each enemy turret the team
  destroys, six at most, eaten on 4: a little health and a moment's speed, 10 s apart.
- **Item actives**: four finished items with one (a shield, a burst of speed, a blast that slows, a
  heal), used on 1, 2, 3, 5, 6 and 7 as League's, their cooldowns their own. One order says both,
  `Order::Use { inv }` (the trinket is slot 6), predicted with the rest of the champion; what they
  do goes out as heals, shields and damage from `Source::Item` and `Source::Treat`, under the same
  per-team rules as every other source's. The HUD shows a slot's key and its seconds, and the
  treats beside the gold.
- **A fifth augment screen**: Encore (Gold) earns one, Prismatic, after the four.
- The server's bots eat treats when hurt and use the actives they buy.
- Rerolls banked across matches for signed-in wallets (two at most, one spent a reroll, one
  earned a match finished; a guest one a select), kept in the records directory.
- The decals ART.md left (cracks across the paving, snow drifted across the seams, in the
  ground's shader on the high tier) and the brush's sway.
Measured: with them alone the bots' matches ran longer and wider (12 seeds: a median of about 19
minutes, three past 28, and one still going at 40, everyone level 18 with six items and nothing to
spend on, the team ahead by 31 kills never ending it). The bots only ever pushed behind their
wave; now, two or more champions up (or the enemy's all down), they press on to the enemy's
foremost structure that can be hit, wave or not. Over 24 seeds every match ends, from 10:43 to
31:45, a median about 15 minutes; the golden bot match (seed 11) ends at 15:04. The golden hashes
hold native and wasm.

Then the screens around a match, which until now were a line of words in the middle: the page
connecting, in the lobby or the queue is a card (the time waited, who else is, the seconds before
the bots, a button in or out of the queue: `ToServer::Leave` had no caller), and a link gone for
good has a Reload; champion select is a screen of its own with the team as badges, the champion
held with its kit's icons and tooltips, the bench as tiles; a match over keeps its scoreboard up
under Victory or Defeat and the player's line, with Play again (the page used to stay on the
result until it was reloaded). On the way: the default font has no `·`, `×` or `–`, drawn as
boxes in the target frame's statuses and beside the treats; a fallen ally's portrait went blank
(its camera drew the body on the ground from chest height), and now keeps the last face, greyed;
the minimap no longer shows over the menus; the ping no longer names the seat.

Then the menu (Esc, or the button at the top left: `menu.rs`), in the game's own UI as Gates'
is (`AnthonE/Gates`, `render/pause.rs` and `settings.rs`), not the page's, so a desktop build has it
too: Controls (the layout, casting, the camera, and every key of the layout chosen), Graphics
(the tier, at the next load, and what's drawing now with its frame rate) and Sound (the page's
panel, `web/audio.js`, until now). Esc is the top thing first: a held cast, the shop, then the
menu. The keys are one table, as Before Colony's are (`el_client_core::controls`): the menu's
page, the HUD's key over each ability and a first match's strip of the essentials read it, and
the input code casts with its keys. Settings were URL parameters, remembered; they still are, and
the menu writes the same memory (`config::remember`: a desktop build keeps them in a file there).

Then the fx stage and the flipbooks (`FX.md`, step 7): one effect fired alone over a dark card and
a light card on a clock stepped 1/60 s a frame, shot into a measured strip by Gates' own shooter
(`scripts/fx.sh`, 296 effects by name, knobs to retune a recipe without a rebuild), and smoke, a
fireball and a hit's flash baked from a fluid simulation and lit six ways by the moon
(`art/tools/flipbooks.py`, one 1024² sheet pair, 460 KB). About fifty effects went through the
loop, one of each kind a family and the world's, and the missiles once the stage could fly one:
a hit's flash as wide as its star was and gone as fast, a buff shown in its first sixtieth, no
soot under a healer's ultimate, a fireball in a family's own colours, a shockwave that's a band
and not a cloud, a cast's flash that isn't pink smoke, dust and foam that don't outlast the hit
they came off, and a missile's head a hard point with a narrow streak behind it, where it was the
pale blob the suite had named. A strip of twelve exact frames takes about a minute in headless
Chromium.

Left, in no order:
- **A name**: a guest's is `?name=` or the server's; the page doesn't ask for one.
- **A real GPU.** Nobody has seen the game on one: the frame rate on either tier (the target is 60
  fps at 1080p on integrated graphics, and a ten-way fight at 60 on `high`), and the effects'
  share of the frame (`FX.md`, step 6).
- **Listening.** The sounds were judged by their spectrograms and measures, not by ear.
- **Tuning by hand**: each ability's look (`FX.md`, step 5; the stage and its strips are the
  loop, `scripts/fx.sh`, and a dozen of the 296 have been through it), each champion's clip
  windows (`CHAMPIONS.md`).
- **Art**: an outline drawn as a line, where the rim stands in (`CHAMPIONS.md`); the HUD's last
  draw calls.
- **The announcer's and the music's files**: wired, optional, dropped in by name (`FX.md`).
- The agents' own team-view bots in the empty seats, where the server's bots sit now.
- **The operator's**: where the game server runs (`--server`, `--http`, `--siwe-domain`), the
  store's route to the page, and the listing.

# Netcode: the netcode of record

This file says what Eloram's multiplayer is built from, what we took from whom, and what we turned
down, so the choices aren't made again each time. `PROTOCOL.md` is the bits (written as they are
built), `DESIGN.md` the mode they carry. Read this before changing the wire, replication, vision or
the record.

Eloram is ARAM: Mayhem: ten players, one lane, about two hundred units, fog and brush, matches of
about twenty minutes, in a browser tab. The operator's brief: what League does, built with the
netcode of a modern MMO. League's posts say what shape it is; Before Colony and Gates say how we
build that shape so it costs nothing a tick; Quake Golf says how to record it.

## What League does

From Riot's own posts (sources at the end). Where Riot says nothing, so do we, and the claim is
marked unverified.

- **The server is authoritative.** "We let the server's game simulation make the authoritative game
  decisions… [we] generally don't trust the information received from the client" (anti-cheat,
  2018). "League of Legends is a server-authoritative multiplayer game, hosted on dedicated servers"
  (Swarm, 2025). The series on determinism says League "does not use the lock-stepped, synchronized
  peer-to-peer networking common in sports, fighting, and traditional real-time strategy games."
- **About 30 Hz, not a fixed step.** The server takes "a measured frame delta time which we
  throttle to a target frame rate", "usually less than 0.033 seconds"; it is "not a fixed-step
  server clock" (Unified Clock, 2017). Riot kept it because the behaviour was established, and
  said a fixed step would have taken frame time out of the inputs. A Riot engineer, 2020: "The
  server tick rate is always 30hz." TFT ran at 20 Hz in some regions and "people were not able to
  tell the difference". A 0.25 s cast lands on the 8th tick (the Wiki).
- **Only the server is deterministic.** "Making 100% of the working set of our game server
  deterministic was not feasible"; the target was the state gameplay depends on. The client is
  not: "the game, unlike the game server, isn't deterministic" (2018).
- **Every frame's inputs are recorded.** The Server Network Recording keeps "network inputs each
  frame in the order in which they're received by the server", captured after decryption as
  gameplay messages, plus the start-up configuration and the measured frame times. Played back,
  it is Chronobreak (an esports game rewound to a moment instead of remade), a bug's repro, and a
  regression test. Playback is not held to real time: "a recovery to 40 minutes often takes less
  than 3 minutes." It took "almost a full year of effort from multiple engineers", April 2016 to
  the 2017 Spring Split.
- **What broke determinism**, in Riot's words and order of cost:
  - Clocks: "8 clock instances with 6 unique implementations", 32-bit float accumulators that
    drifted past about 4,000 s, "magic number multipliers to correct for drift". Fixed by one
    clock: "elapsed time and absolute time are both sampled at the start of each frame, just before
    gameplay processing begins." Six months.
  - Uninitialised memory: "The 3-coordinate floating point vector class in our game does not
    initialize itself to zeroes due to the performance cost." The bug they hunted in the last post
    (1 game in 5,000, found by bisecting 580,000 constructions) sat in the network level-of-detail
    code that decides whether a minion's path update goes to a player.
  - Random numbers: several generators, some shared with code that wasn't deterministic. Replaced by
    one XOR-shift generator seeded from the game's id.
  - Iteration order: a `std::map` keyed by pointers; "very hard to intuit that this causes a
    divergence by static code inspection."
  - Threads: minor. The OS version: some. Floats: a risk, never a bug they found.
- **How they find a divergence.** Checksums say that a run diverged, never where, so Riot logs
  "game state as a set of hierarchical key-value pairs", the changed values only, at the end of
  each frame, and a diff names the first difference, since "only the first divergence really
  matters." 2–3 thousand real games a day from the beta servers. "The price of determinism is
  eternal vigilance."
- **Fog is the server's.** "We don't share the state of other players if it doesn't need to be
  shared… so we can avoid common cheats like "map hacks"" (2018). "If you couldn't see an enemy,
  the server wouldn't tell you where they were" (2020). Vision is a 128×128 grid over the map,
  scaled to 512×512 to draw (2016). On top of vision, minion path updates are filtered by where a
  player's camera is.
- **Movement is waypoints.** "each unit receives its path in the form of waypoints… Both client and
  server smoothly interpolate the unit's position along the path in a deterministic way." The
  client's easing near a path's end made units speed up and slow down in Swarm, "an obvious desync
  between client and server", and Riot simplified it (Swarm, 2025).
- **Prediction is opt-in.** The setting "Use movement prediction" "smooth[s] out movement inputs
  from the player while the connection is unstable or at high latency" (the Wiki; Riot has written
  nothing on it). Dota 2 has none: "Dota does not perform client-side prediction other than input
  confirmation" (Valve, 2025).
- **Small datagrams, and where the limit is.** Game packets are "around 55 bytes" (Riot Direct,
  2015). The machines' real limit was "how many games we can run on a machine before the network
  threads' overhead was maxed out", about 200 a machine; four players cost about 8× one (Swarm).
  "Still very playable at around 60ms" (2021).
- **Spectators watch 60 s behind** on Howling Abyss, 180 s on Summoner's Rift, "to uphold the
  competitive integrity… and discourage ghosting" (the Wiki).
- **Unverified:** how skillshots meet latency (no Riot source; whether anything is rewound is
  unknown), bandwidth a player, when movement prediction arrived, the transport (ENet and Blowfish
  come only from 2012 conference slides).

## Our three netcodes

| | Before Colony | Gates | Quake Golf |
|---|---|---|---|
| model | authoritative; the owner predicts, everyone else is interpolated | the same, with the client's clock steered by the server | deterministic lockstep: only inputs cross, a relay closes ticks, a referee re-steps them |
| tick | fixed 30 Hz | fixed 30 Hz | fixed 125 Hz |
| what's hidden | what the suit's sensors don't detect (`Sim::detects`) | outside an interest radius; occlusion planned | nothing: "every machine holds the whole world" |
| wire | acks on every datagram; per-entity deltas against acked baselines (78–104 bits a suit, 30 at rest); priority accumulator; ≤1,100 B, never fragmented | the same classes (dynamic, structural, global); Q3's zero baseline | one u32 word a player a tick, changed words Exp-Golomb coded; 8.6 B a tick for 8 fighters |
| prediction | replay unacked commands on every snapshot; p99 0.1 mm at 100 ms RTT and 5% loss | a decay ramp when inputs starve; a consume throttle that executes both frames; corrections blended | your body re-stepped each frame from the confirmed world; others a round trip late |
| time | input lead steered on input health; interpolation 2–6 ticks | the client ahead by RTT/2 + 1–2 ticks on a ±5% proportional controller; interpolation 2–8 ticks, adaptive | input lead steered on the relay's slack; a playout clock at ±3% |
| cost | 448 pilots in one fight: 12.8 / 20.1 ms p50 / p99 of a 33 ms tick; 0.2 µs a record; the tick never allocates | 100 clients: 0.8 ms a tick | 256 players: 0.2–0.4 ms of an 8 ms tick; 9 KB/s down |
| record | planned, unbuilt | a WAL | the relay's record plus the referee's interval hashes: replays, spectator trees, failover with every slot kept |

**Before Colony gives the wire and the hot path.** Its tick is allocation-free and lock-free, proved
by counting the heap, and it already sends deltas against acked baselines under a budget. A MOBA
is a small version of its worst case: ten clients and about two hundred units, against 448 pilots
in one fight.

**Gates gives time.** The client's clock runs just ahead of the server, steered on the server's own
buffer gauge, and a starved tick fades movement rather than repeating it at full stride. Its rule
for corrections is the operator's: the player never sees one.

**Quake Golf gives the record.** Its referee steps the same build as the clients and names the slice
a desync got wrong; a replay is the same ticks a client steps, with the network taken away; a
standby takes a room over on the old clock's timeline. Riot built the same thing for Chronobreak.

## What we build

### The law: one tick, deterministic

- **Fixed 30 Hz.** League's rate, with the fixed step Riot said they wanted. Every timer is an
  integer count of ticks.
- **A closed function of the seed and the inputs.** Trig through `libm` (`el_sim::math`), no
  `fast-math`, no wasm `simd128`, no `mul_add`. One generator, seeded from the match. One clock,
  read once at the top of the tick. Storage sized at construction, so nothing is ever
  uninitialised. Iteration in slot order, never by address. That is Riot's list of what broke
  them, ruled out from the first line.
- **Golden hashes** over a whole twenty-minute bot match, native and wasm, in CI.

### Inputs: a small command, and orders

- **The command** rides every tick, as Before Colony's does: a WASD direction for those who steer,
  held keys, and the camera's centre (used for priority only, never for what you're shown; one a
  packet, the newest command's). Each input packet repeats the last four. A packet goes when a
  tick's command is made or an order is given, so 30 a second whatever the frame rate (a frame
  between two ticks has nothing new to say). A slow frame that makes more than four sends as many
  packets as carry them all, the newest first (`Client::poll`; the page raises the browser's
  datagram high-water mark to fit them), since a command never sent is a step the prediction took
  and the server didn't. The orders given in a frame are stamped for the newest tick it makes, not
  its oldest, which on a slow frame is already the server's past.
- **Orders** are what a MOBA player gives: move, attack-move, attack a unit, cast a slot (pressed
  and released, at a point, a unit or a direction), stop, hold, level a skill, pick an augment,
  reroll one, buy, sell; and to the team, a ping and a vote to surrender (`el_sim::voice`). Each
  carries a sequence number and repeats in every packet until
  acked, and the server applies each exactly once, at its stamped tick or the next. A stalled
  client's champion keeps doing what it was last told, because intent (where to go, whom to hit)
  lives in the sim, not in the input.
- **A missing command** is stood in for by repeating the steer and nothing else, fading out after a
  few ticks (Gates' decay). An order is never repeated by the server.

### Your champion is predicted

- The owner steps its own champion with the server's own function (`champ::own_step`), so a click
  moves you on the next frame: the path, dashes, blinks, wind-ups and your own missiles' launch.
  League makes this optional; we make it the default.
- Each snapshot reseeds the prediction from the server's state of your champion, and the unacked
  commands and orders are replayed on it. That state is one fixed list of fields, floats as their
  bits (`el_sim::own::OwnState`), sent against the newest the client acked (`el_proto::fields`: a
  bit a group of eight unchanged, else a bit a field, and a changed 32-bit field as the significant
  bits of its XOR with the baseline's): what changed in the tick, to the bit, about 18 B where the
  whole is about 210. A difference is blended out over a few frames (Gates'
  0.95 a frame for small errors) and snapped only past a few hundred units.
- The client's clock runs ahead of the server by RTT/2 and one or two ticks, steered on the
  buffer depth the server reports in every snapshot (Gates, after Overwatch).

### Everyone else is interpolated

- Two to eight ticks back, breathing with the measured jitter.
- Each unit's record carries its next path corner. Extrapolation then turns the corner instead of
  running through a wall. This is League's waypoints at the price of one field that is almost
  always "same".

### Closed forms go once

- **A missile is sent once.** Its origin, direction, speed and range from the tick it was cast, in
  one event. The client draws it on its *predicted* clock, where your own champion is, so a dodge
  you see is a dodge the server sees.
- **Hits are judged on the server's present.** Targeted autos, targeted spells and tower shots are
  homing missiles locked to a unit, so they cannot miss and need no rewind. Nothing is rewound.
- **Minion waves are a closed form of the clock** until something touches them.

### Fog is the server's, a team at a time

- Vision is computed twice a tick, once a team (two answers a unit, not ten), on a grid as fine as
  League's: cells of 120 units, stamped from line-of-sight spans precomputed for the map.
- A brush is a region. A unit in brush is seen by the other team only from inside that brush, or
  when revealed.
- A team is sent its allies and what it sees, nothing else. When sight is lost, a `Leave` event
  goes out.
- **Every event has a team rule, with no catch-all.** A missile from a caster you can't see first
  appears where it enters your vision, with no owner, so it cannot place its caster.
- **A team's word is its own.** A ping goes to the pinger's team, naming a unit only if the team
  sees it (when it's given, and when it's sent); a vote to surrender, as it stands and as it
  closes, to the voting team. The enemy hears only the outcome. Who may vote is who holds a seat,
  as the record's seat frames say, so the referee counts the same votes.
- The client draws fog from the same precomputed table, so the fog on screen is exactly the fog the
  server enforces.
- A test decodes every datagram every client gets and fails if any record or event names an enemy
  the server's vision didn't show that team on that tick.
- **The seed stays on the server.** Every seat's augment offers follow from it, so a player is told
  only what it decides for everyone: the augment screens' tiers and the headliner
  (`ToClient::Match`; `Match::told` builds the client's copies). A seat's own offers come in its
  own section. Champion select shows a team its own champions and bench only.
- **Nor does the record give offers before their time.** A live match draws its offers from a
  secret the server keeps (`Control::Start`'s, from the system's randomness), not from the seed in
  the record's header. The record gets each screen's key in a frame as the screen opens, so a
  watcher a minute behind learns an offer a minute after its player did, and the header alone
  replays to other offers (`el-match/tests/record.rs`). A match without a secret (the tests', a
  record from before) draws from the seed as it always did.

### The wire

Before Colony's, sized for a match:
- An ack on every datagram (a sequence and the 32 before it as bits). The ack names losses too: a
  client takes nothing older than the newest datagram it has, so one the server sent before a
  datagram acked, and not acked itself, is lost for good, and the server knows it a round trip on.
- Per-unit deltas against the last state the client acked, in integer codes both ends agree on to
  the bit, falling back to a whole record from the same encoder. The seat keeps each unit's acked
  record however old (and the client each unit's newest 16), so a unit that says nothing new for
  minutes is still a delta; a delta's age is 5 bits, or 17 past 16 ticks.
- **A record goes once, and once more.** A unit's record is sent when it says something its last
  one didn't (a unit at rest, or walking straight, says nothing), then again the next tick it can
  be, so one datagram lost costs nothing; after that only if the ack names the datagram that held
  it lost, or none comes for 16 ticks. Gaffer's way sends the newest of everything every tick until
  an ack; at 100 ms that is each change four or five times, which was three bytes in four.
- Events: the reliable ones (a unit leaving sight, a missile's or a zone's start and end, deaths,
  casts, levels, pings, votes) repeat until a datagram holding them is acked; their ticks go as
  ages from the snapshot's (a zone's next pulse and end either side of it). A zone is a closed
  form as a missile is, told where the team sees its ground; one that follows its owner, only while
  the team sees the owner (`docs/FX.md`, "The wire").
- A priority accumulator under a byte budget.
- Every datagram ≤1,100 B, never fragmented (at 1% loss a 10-fragment packet is lost 9.5% of the
  time).
- Measured over 100 ms, ±20 ms and 5% loss with eight bots and two clients playing
  (`el-match/tests/netcode.rs`): 3.7 KB/s down a client, the largest datagram 450 B, 0.8 KB/s up;
  what the client takes a unit to be is what the server's record of it said at the snapshot's
  tick for 99.9% of the units its team sees. The estimate was 18–27 KB/s.

### The record

- Each match writes one file (`el_sim::record`, `.elr`): its seed, champions, protocol and content
  version, then each tick's *applied* inputs for all ten seats (the bots' too), everything from
  outside (a seat taken or left, an augment screen's key as it opens), and a state hash every 30
  ticks. A referee steps a tick once its frames are in (the keys come after the step that drew them,
  so a tick waits for the next frame that isn't one). The shard thread writes the frames into a ring
  a match without allocating; a recorder thread a shard keeps them (`el-server/src/records.rs`;
  `--records dir` writes them out). Measured: 25 KB a match a minute with two people and eight bots,
  about 500 KB for twenty minutes.
- **A referee** (`el_sim::record::Referee`) re-steps a record and checks every hash: `el-replay
  verify` over files, every spectator's page as it watches, and the server itself, live. A
  divergence is named to the 30 ticks it happened in; one order changed in a record is caught at the
  next hash (`el-match/tests/record.rs`).
- **A checkpoint** (`el_sim::snapshot`) is a whole match as bytes: every field of every slot, the
  dead ones too, so a match read back steps exactly as the one written (the hash covers less; the
  tests step both for minutes and compare every tick and the bytes at the end). Little-endian,
  floats as their bits, enums padded: the same bytes native and wasm (a golden digest on both), and
  the same length for every match, 156 KB (5.6 KB gzipped, as `/records/{id}/checkpoint` serves it),
  written in about 240 µs and read in 300. Never the offer secret. Each type is destructured field
  by field, so a field added to the match doesn't compile until the checkpoint has it.
- **The live referee.** Each match's shard writes its checkpoint once a minute
  (`--checkpoint-secs`), just after a hash, each match a hash later than the one before so a shard
  writes one a tick at most, onto a ring of its own (about 240 µs each: the shard bench, 64
  matches, has its p99 at 23.6 ms with them and 23.9 without, 90 and 89 matches a core). The recorder thread re-steps every live record as it takes it, outside the store's lock
  (about 83 µs a tick, a match's sim again: with 40 agents in four matches, 18,012 ticks re-stepped
  in 1.5 s of a core, 600 hashes and 11 checkpoints checked, none astray), and at a checkpoint's
  tick compares its own match with the shard's, byte for byte (`el-server/src/referee.rs`). Riot's
  rule holds: a checkpoint is only a cache of the record. One is served or recovered from only once
  the referee has found it the same; one that isn't is named to its unit and field ("unit 37 (melee
  minion, team 1): stats.armor differs"), after the hash that first went wrong, since "only the
  first divergence really matters". `/status` has the referee's ticks, hashes, checkpoints and
  divergences; `/records.json` a record's checkpoint and any complaint.
- **Spectating** is the record served a minute behind its players (`/records/{id}`: a live record is
  cut at the last hash at least 60 s old), stepped in the browser by the same sim with full vision
  (`?watch=<id>`; `/records.json` lists them). The delay is the server's, so it can't be skipped.
  The page starts from the last verified checkpoint inside what's served
  (`/records/{id}/checkpoint`) and checks every hash after it, catching up at 900 ticks a frame.
- **A replay** is the same from tick 0 (`?replay=<url>`; `[` and `]` change the speed), and any
  recorded match re-runs far faster than real time: `el-replay verify` re-steps a minute of play in
  about 0.1 s, and resumes each record's checkpoint (`{id}.ck`) to the same end.
- **A crash** is recovered as Chronobreak recovers one: the record re-stepped, not memory restored
  (Riot tried save-states of game memory first, and they "failed to give esports officials the
  precise tools they needed"). With `--records dir` a live match's record is written as it comes
  (`{id}.elr.part`, whole frames only), with its last verified checkpoint (`{id}.ck`) and what its
  players were told with their rejoin tokens' hashes (`{id}.live`). A server started on that
  directory re-steps each part from its checkpoint to its last hash and cuts it there (at most a
  second lost), puts the match in a free place with its bots in every seat, and keeps it a minute
  for its players; the page and the agents dial again on their own and take their seats back with
  their tokens, as after any dropped link (four matches two and a half minutes in, killed with
  SIGKILL, were taken up by the next server in under half a second, 56 to 83 ms each, about 120
  bytes of record lost each). The keys already drawn are in the record; a screen not yet opened
  draws from a new secret. The self-signed certificate and the resume tokens' hashes are kept there
  too, so pages pin the same hash and signed-in players resume. Only the newest checkpoint is kept
  on disk, so a match taken up is watched from its first tick until the referee has verified one a
  minute old. Stopped (Ctrl-C), a server leaves its matches the same way, for the next one to take
  up. `el-server/tests/recover.rs` SIGKILLs a real server mid-match, and the one record that results
  re-steps from its first tick to its last; `scripts/e2e.sh recover` does it under the page.
- **A match that panics** is stopped alone (each match's tick is unwound to, so its shard's other
  matches go on), and its players told it was no contest. A shard whose thread stops ticking is
  taken as gone: its matches end, and the lobby puts no more there.
- CI re-steps every match it records and fails on the first differing hash.

### Many matches a process

- One sim thread a core, each stepping a pool of matches built at start-up and reset in place, so
  nothing is built or dropped on the hot thread.
- Each pool has an egress thread and a recorder, woken by `unpark()`. Threads meet only through
  preallocated lock-free queues (`rtrb`, `ArrayQueue`). The match tick never allocates or locks.
- The budget a match tick is about 300 µs at p99, half simulation and half replication, which
  makes at least 64 matches a core.
- The lobby, matchmaking, sign-in and HTTP run on tokio, off the hot path.
- An egress ring holds `[u16 len][datagram]` frames, each written in one commit and read only
  once all of it is there: the egress thread reads while the shard writes, so a length must never
  be seen without its datagram.
- No client can stall its session. The control stream is written by a task of its own through a
  bounded outbox, every write has a deadline, and a client that stops reading is let go. Lobby
  requests are rate-limited.
- A seat outlives its connection. Each player's `Match` carries a rejoin token, and a new
  connection takes the seat back with it while the match lasts (`ToServer::Rejoin`). If the old
  connection still holds the seat it is replaced; until then the server's bot plays the seat. A
  match with nobody connected waits 60 s for its players. The page redials on its own when the
  link closes or goes quiet for 4 s, and a reload rejoins with the token the tab kept.
- Empty seats take bots. A bot is the client's own netcode with a brain, so it plays through the
  same orders, sees the same team's view, and is in the record like anyone. The store is for
  people and agents equally.

## Turned down

- **Lockstep.** Quake Golf puts 256 players in a room for 9 KB/s and its record is free, but every
  client holds the whole world. Brush and fog would be a memory read, the client's exact future
  would make dodge scripts trivial, and what an enemy does to you would arrive a round trip late.
  The RTS games that ran this way had maphacks for exactly that reason (OpenConflict, 2011), and
  the genre left it when it left Warcraft III. Quake Golf's own roadmap lists hidden information as
  what its engine can't do.
- **Whole paths on the wire.** League sends them. Under fog each team would need its own truncated
  copy, minions repath every time their target changes, and crowd control and dashes break the
  closed form. The next corner in a delta costs less.
- **Rewinding for skillshots.** Missiles are simulated on the server and drawn on the client's
  predicted clock; targeted things lock by id. There is nothing to rewind (Gates' finding for
  arrows).
- **A variable step.** Riot kept theirs because of legacy behaviour and said why it cost them.
- **Peer to peer.** The worst peer sets the game, and no one can be trusted with the fog.
- **Vision a viewer at a time.** Teammates share sight, so there are two answers, not ten.
- **Fragmented datagrams**, and state too big for one: big things (a joiner's state, a spectator's
  checkpoint) go on a reliable stream.

## What we watch

- A match tick's split, simulation and replication, p50 and p99, and matches a core, on the bench
  from the first commit.
- `hot_path_allocations` at 0, from the counting allocator in production.
- Prediction error by cause (chasing, your own dash, an enemy's crowd control, bodies), through a
  link with 100 ms RTT, 20 ms jitter and 5% loss.
- Bytes a client a second, and records a datagram.
- What a client knows of its team's view: the units whose newest record, carried on, isn't what
  the server's said at the snapshot's tick, under half a percent over 5% loss (the netcode test).
- Vision leaks: zero, by the test.
- The referee's divergences: zero.

## Sources

**Riot**
- Determinism in League of Legends: [Introduction](https://www.riotgames.com/en/news/determinism-league-legends-introduction) (2017),
  [Implementation](https://www.riotgames.com/en/news/determinism-league-legends-implementation) (2017),
  [Unified Clock](https://www.riotgames.com/en/news/determinism-league-legends-unified-clock) (2017),
  [Fixing Divergences](https://www.riotgames.com/en/news/determinism-league-legends-fixing-divergences) (2018).
  The same posts are technology.riotgames.com nodes 67, 68, 73 and 84.
- [Minions, Servers, and Progression – The Tech Behind Swarm](https://www.riotgames.com/en/news/the-tech-behind-swarm) (2025).
- [Riot's Approach to Anti-Cheat](https://www.riotgames.com/en/news/riots-approach-anti-cheat) (2018).
- [Demolishing Wallhacks with VALORANT's Fog of War](https://www.riotgames.com/en/news/demolishing-wallhacks-valorants-fog-war) (2020).
- [A Story of Fog and War](https://www.riotgames.com/en/news/story-fog-and-war) (2016).
- [Profiling: Real World Performance of League](https://www.riotgames.com/en/news/profiling-real-world-performance-league) (2018).
- [Leveling Up Networking for a Multi-Game Future](https://www.riotgames.com/en/news/leveling-networking-multi-game-future) (2021).
- Fixing the Internet for Real Time Applications, [Part I](https://technology.riotgames.com/node/25) (2015),
  [Part II](https://www.riotgames.com/en/news/fixing-internet-real-time-applications-part-ii) (2016),
  [Part III](https://technology.riotgames.com/node/43) (2016).
- [Artificial latency for remote competitors](https://lolesports.com/en-US/news/riot-games-tech-blog-artificial-latency-for-remote-competitors) (2022).
- The League Wiki: [Tick and updates](https://wiki.leagueoflegends.com/en-us/Tick_and_updates),
  [Spectator](https://wiki.leagueoflegends.com/en-us/Spectator),
  [Settings](https://wiki.leagueoflegends.com/en-us/Settings).
- A Riot engineer on the 30 Hz tick: [devtrackers](https://devtrackers.gg/leagueoflegends/p/c9fe6016-can-anyone-confirm-if-the-way-autoattacks-wind-up-has-changed) (2020).

**Others**
- Valve on Dota 2's prediction (2025), through press copies.
- Bettner and Terrano, [1500 Archers on a 28.8](https://www.gamedeveloper.com/programming/1500-archers-on-a-28-8-network-programming-in-age-of-empires-and-beyond) (2001).
- Forrest Smith, [Synchronous RTS engines and a tale of desyncs](https://www.gamedeveloper.com/design/opinion-synchronous-rts-engines-and-a-tale-of-desyncs) (2011).
- Bursztein et al., [OpenConflict: preventing real time map hacks](https://www.ieee-security.org/TC/SP2011/PAPERS/2011/paper032.pdf) (IEEE S&P 2011).
- Glenn Fiedler, Gaffer On Games, and the GDC talks on Overwatch (2017) and Rocket League (2018), as
  cited in Before Colony's and Gates' netcode docs.

**Ours**
- Before Colony (`AnthonE/Before-Colony`): `docs/NETCODE.md`, `docs/SCALE.md`, `docs/PROTOCOL.md`.
- Gates (`AnthonE/Gates`): `docs/NETCODE.md` (§3 the hot pipeline, §4 time, §8 lag compensation).
- Quake Golf (`AnthonE/Quake-Golf`): `README.md` (the relay, the referee, N1, N3, N4), `docs/wire.md`,
  `docs/roadmap.md`.

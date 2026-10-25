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
  held keys, and the camera's centre (used for priority only, never for what you're shown). Each
  input packet repeats the last four.
- **Orders** are what a MOBA player gives: move, attack-move, attack a unit, cast a slot (pressed
  and released, at a point, a unit or a direction), stop, hold, level a skill, pick an augment,
  reroll one, buy, sell, ping. Each carries a sequence number and repeats in every packet until
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
  commands and orders are replayed on it. A difference is blended out over a few frames (Gates'
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
- The client draws fog from the same precomputed table, so the fog on screen is exactly the fog the
  server enforces.
- A test decodes every datagram every client gets and fails if any record or event names an enemy
  the server's vision didn't show that team on that tick.

### The wire

Before Colony's, sized for a match:
- An ack on every datagram (a sequence and the 32 before it as bits).
- Per-unit deltas against the last state the client acked, in integer codes both ends agree on to
  the bit, falling back to a whole record from the same encoder.
- A priority accumulator under a byte budget.
- Every datagram ≤1,100 B, never fragmented (at 1% loss a 10-fragment packet is lost 9.5% of the
  time).
- About 100 units in sight fit in one datagram a tick. The estimate is 18–27 KB/s down a client.

### The record

- Each match writes one file: its seed, content and build, then each tick's *applied* commands and
  orders, everything from outside (joins, leaves, reconnects), and a state hash every 30 ticks.
  About 250 KB for twenty minutes.
- **A referee** re-steps every live match about a minute behind, checks the hashes, and keeps a
  checkpoint every minute. A divergence is caught within a minute and named to the tick and the
  unit (Riot's first-divergence tool).
- **Spectating** is the checkpoint at least 60 s old plus the record from there, stepped in the
  browser by the same sim with full vision. The delay is the server's, so it can't be skipped.
- **A replay** is the same from tick 0, and any recorded match can be re-run faster than real time.
- **A crash** is recovered from the last checkpoint and the record, and clients resume their seats.
- CI re-steps every match it records and fails on the first differing hash.

### Many matches a process

- One sim thread a core, each stepping a pool of matches built at start-up and reset in place, so
  nothing is built or dropped on the hot thread.
- Each pool has an egress thread and a recorder, woken by `unpark()`. Threads meet only through
  preallocated lock-free queues (`rtrb`, `ArrayQueue`). The match tick never allocates or locks.
- The budget a match tick is about 300 µs at p99, half simulation and half replication, which
  makes at least 64 matches a core.
- The lobby, matchmaking, sign-in and HTTP run on tokio, off the hot path.
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

# Eloram — notes for AI assistants

ARAM: Mayhem in a browser tab: five against five on one lane, random champions, augments as you
level. Rust server, Bevy 0.19 client compiled to wasm, WebTransport (QUIC) between them. Built on
Before Colony's patterns (`AnthonE/Before-Colony`) and listed on the Elo Pros store.

- `docs/NETCODE.md` is the netcode of record: what League does (from Riot's posts), what we took
  from Before Colony, Gates and Quake Golf, what we turned down. Read it before changing the wire,
  replication, vision or the record.
- `docs/DESIGN.md` is the mode: the rules at League's numbers, in our own names.
- `docs/ROADMAP.md` is the phases.

## Rules (non-negotiable)

- **The match tick does not allocate or lock** (the sim step, the input drain, replication, the
  record). Storage is sized at construction (`Box<[T]>`); `el-sim/src/storage.rs` is the only
  allocation site in the sim. `el-proto` is `no_std` with no `alloc`.
- `el-sim` and `el-match` have `clippy.toml` bans (Mutex/RwLock/String/HashMap/`vec!`/`format!`/
  `Box::new`…). Threads talk only through preallocated lock-free queues (`rtrb`, crossbeam
  `ArrayQueue`); a shard thread never wakes tokio, it `unpark()`s its egress thread.
- Proof: the `no_alloc` tests count heap operations with `el-alloc::CountingAlloc` and stay at 0.
- **Determinism**: trig through `el_sim::math` (libm); never glam `fast-math`, wasm `simd128` or
  `mul_add`. One generator from the match seed, one clock read per tick, timers as integer ticks,
  iteration in slot order. The golden hashes hold native and wasm.
- **The server decides.** Clients send commands and orders, never positions. An order carries a
  sequence number and is applied exactly once.
- **Fog is the server's, a team at a time.** A team is sent its allies and what it sees, nothing
  else. Every event has an explicit per-team rule, never a wildcard arm. The vision-leak test
  decodes every datagram and stays at zero.
- **No lag compensation.** Missiles are closed forms sent once and drawn on the predicted clock;
  targeted things lock by id; hits are judged on the server's present.
- **The record is the truth.** Replays, spectating, the referee and crash recovery are all the
  record re-stepped; anything that changes the sim and isn't in it is a bug.
- Every name is the game's own: no champion, map, item or augment name of Riot's.
- Never log signatures or resume tokens.

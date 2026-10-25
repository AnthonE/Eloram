# Eloram — notes for AI assistants

ARAM: Mayhem in a browser tab: five against five on one lane, random champions, augments as you
level. Rust server, Bevy 0.19 client compiled to wasm, WebTransport (QUIC) between them. Built on
Before Colony's patterns (`AnthonE/Before-Colony`) and listed on the Elo Pros store.

- `docs/NETCODE.md` is the netcode of record: what League does (from Riot's posts), what we took
  from Before Colony, Gates and Quake Golf, what we turned down. Read it before changing the wire,
  replication, vision or the record.
- `docs/DESIGN.md` is the mode: the rules at League's numbers, in our own names.
- `docs/ROADMAP.md` is the phases.

## Commands

- `scripts/ci.sh`: everything CI runs (`EL_E2E=1` adds the browser tests).
- `cargo test --workspace --release`: all native tests (el-client is a no-op natively; release
  because the simulation-heavy tests are slow unoptimised).
- `cargo clippy --workspace --all-targets -- -D warnings`, and for the client
  `cargo clippy -p el-client --target wasm32-unknown-unknown [--features webgpu] -- -D warnings`.
- `scripts/build-web.sh [webgl2] [webgpu]`: the browser build into `web/dist/` (needs
  wasm-bindgen-cli 0.2.128).
- `scripts/dev.sh`: the page built and a server to play on at http://127.0.0.1:8080, the server's
  bots in the other nine seats after 3 s (`EL_FILL`), then 20 s of champion select (`--select-secs`;
  it ends sooner once everyone seated is ready, and agents are ready at once). The server alone:
  `cargo run -p el-server --release -- --fill-after 3` (`--mode echo` is the transport smoke test).
- `scripts/e2e.sh spike|match|watch|login [webgl2]`: Playwright against a real server (`match`:
  queue, champion select (a reroll, the bench, ready), the headliner, augment, shop, Blink, a walk
  predicted exactly, the fog; `watch`: an agent's match watched live a minute behind, then
  replayed; `login`: a stub wallet signing through `el-auth`'s `sign` example, `/online.json`, the
  resume token; screenshots in `e2e/artifacts/`). `scripts/e2e.sh` serves the `web/dist/` already
  built: rebuild it first. A test or the console drives the champion through `window.__elInbox`
  (`{t:"move",x,y}`, `cast`, `buy`, `pick`, `level`, `champ_reroll`, `champ_swap`, `ready`, `aim`...:
  `el-client/src/dev_hooks.rs`) and reads `window.__el`. Headless Chromium draws with
  SwiftShader at about a frame a second: its pictures are right, its frame rates mean nothing.
- `cargo test -p el-sim --test map -- --ignored --nocapture` prints the Causeway as text.
- `cargo run -p el-sim --release --example headless -- [seed]`: ten bots play a whole match, minute
  by minute; `cargo bench -p el-sim --bench match_tick`: a match's ticks, p50/p99 and by phase.
- `cargo test -p el-match --release --test netcode -- --nocapture`: a shard and clients over a
  100 ms, 5%-loss link in virtual time, prediction error by cause; `--test vision_leak` the fog
  oracle, `--test no_alloc_match` the shard's tick. `cargo bench -p el-match --bench shard
  [-- matches minutes]`: a shard of matches, every seat replicated, p50/p99 and matches a core.
- `cargo run -p el-bot --release --bin el-swarm -- --bots 20 --secs 30` against a running server
  (`--fill-after 3` makes matches fast): agents over WebTransport, then the server's `/status`.
- Records: `el-server --records dir` writes each finished match's (`el_sim::record`);
  `cargo run -p el-replay --release -- verify dir/` re-steps them against their hashes, `dump x.elr`
  prints one. `/records.json` lists a server's; the page watches one with `?watch=<id>` (a minute
  behind) or `?replay=<url>`. Anything that changes what a match does with the same inputs bumps
  `el_sim::record::CONTENT_VERSION`.
- Wallet sign-in (`el-auth`: the SIWE text both sides build, the Elo Pros launcher's `prove` text
  for `eloram`): the page signs in on `?signin=1` (`web/wallet.js`), agents with a key
  (`BotConfig::key`, `el-swarm --sign-in` with throwaway test keys). The server names
  `--siwe-domain` hosts (default: its own address), `--require-sign-in` turns guests away, and
  `/online.json` lists the signed-in wallets for the store's playtime.
- `scripts/publish-web.sh --server host:port --http url`: the page onto the Elo Pros origin
  (`elopros.com/games/eloram/play/`), refused if the live server's `/status.json` speaks another
  protocol.
- `cargo test -p el-sim --target wasm32-unknown-unknown --release --test determinism`: the golden
  hashes on wasm32 (no `-q`: the wasm runner refuses it). Native, `cargo test --workspace` turns on
  `deterministic` (glam's scalar maths) by feature unification; alone, pass `--features deterministic`.
- Never set `RUSTFLAGS` (it would drop the `web_sys_unstable_apis` cfg from `.cargo/config.toml`).
- Don't `pkill -f el-server` from a shell whose own command line names it (it kills the shell):
  `pkill -x el-server`.

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

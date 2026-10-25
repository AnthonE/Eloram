# Eloram

ARAM: Mayhem in a browser tab. Five against five on one lane, a random champion each, augments
picked as you level, about twenty minutes, first nexus to fall loses.

- An authoritative Rust server at a fixed 30 Hz, deterministic, many matches to a core, a tick that
  never allocates.
- Fog on the server: each team is sent only what it sees.
- Your champion moves on the next frame; everyone else is drawn a few ticks back; skillshots are
  drawn where your champion is, so a dodge you see is a dodge.
- Every match is recorded: replays, a spectator feed 60 s behind, and recovery from a crash all
  play the record back.
- A Bevy client compiled to wasm, over WebTransport.

`docs/NETCODE.md` is how and why, from Riot's own posts on League and our three other games
(Before Colony, Gates, Quake Golf). `docs/DESIGN.md` is the rules. `docs/ROADMAP.md` is the order
it is built in.

MIT licensed.

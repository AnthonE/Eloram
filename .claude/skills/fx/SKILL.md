---
name: fx
description: Make or tune a visual effect in Eloram (a hit, a blast, a cast, a buff, a dash, a missile in flight, a status, a death, the relic) on the fx stage - stage it alone in front of a dark and a light card, read its filmstrip and numbers, retune it with knobs without a rebuild, compare against the last run, and write the numbers that work into the recipe. Use when asked to add, fix, tune, compare or review an effect or a particle look.
---

# Effects in Eloram

An effect is judged as it moves, never from a single still. The loop is:
1. stage the effect alone;
2. read its **strip** (every frame, its envelope and its numbers);
3. change one thing;
4. read the strip again and its **diff** against the last run.

The loop is Gates' (`AnthonE/Gates`, `crates/fx/src/stage.rs`, `ci/fxstrip.mjs`, its `fx` skill);
the shooter is vendored as `e2e/fxstrip.mjs`. Eloram's particles are its own: closed forms moved
in the shader (`docs/FX.md`, step 2), not Gates' pool. The catalogue, the clock and the knobs are
`el_client_core::fx::stage` (tested natively); the page is `el-client/src/stage.rs`.

```bash
scripts/fx.sh list [filter]                        # every effect the stage can fire (296)
scripts/fx.sh knobs                                # every knob a recipe is retuned with
scripts/fx.sh orrin-q-impact                       # → e2e/artifacts/fx/orrin-q-impact/{strip.png,numbers.json,diff.png}
scripts/fx.sh orrin-q-impact --knobs 'impact.star=1.5;size=0.8'   # retune, no rebuild
scripts/fx.sh orrin-q-impact --seed 7 --frames 15 --span 1.2      # another roll, more frames, a longer span
scripts/fx.sh orrin-q-impact --side enemy --size 300 --low        # as an enemy sees it, 300 damage, the low tier
scripts/fx.sh --no-build …                         # the page as it was last built
EL_FX_DIST=dir scripts/fx.sh --no-build …          # a copy of web/dist, while web/dist is rebuilt
```

**Read `strip.png` first.** Its parts:
- **The hero frame:** the brightest frame, drawn large.
- **The envelope:** how much each card changed over time; the last run is dashed.
- **Every frame:** captioned with its step, time, live bursts (light/smoke) and per-card numbers.

Then read `numbers.json` for anything the picture leaves open, and `diff.png` after a change.

Setup and speed:
- `scripts/fx.sh` builds the WebGL2 page (`scripts/build-web.sh`: wasm-bindgen-cli at the lock's
  version; about four minutes a rebuild, which is why the knobs exist), serves it itself and drives
  `?fxstage=<name>` in headless Chromium on SwiftShader. Playwright comes from `e2e/` (`npm ci`);
  no game server.
- A strip of twelve frames over a second and a half takes about a minute. The clock holds for
  every frame, so SwiftShader's frame rate doesn't matter: the frames are exact.
- A rerun with nothing changed shows 0% moved.
- A long batch: snapshot the page (`cp -r web/dist DIR`) and shoot with `EL_FX_DIST=DIR`, so a
  rebuild meanwhile doesn't pull it from under the shooter.

## The stage

- **One effect, fired twice at once** (`stage.rs`), laid on the Causeway's middle with everything
  else hidden:
  - in front of a **dark card** (the frame's left half; light reads here);
  - in front of a **light card** (the right half; smoke, dust and a scorch read here).
- **The camera:** the game's own rig, close over the cards, looking at the effect's own height
  (`Staged::lift`: a hit is drawn on a body's near side, a metre and a half up).
- **The look:** the game's moon and night, TonyMcMapface, no vignette; the high tier on WebGL2
  (shadows, MSAA, no bloom), `--low` for the low tier.
- **The clock** steps exactly 1/60 s a frame (`stage::Clock`, run in `First` before time moves):
  step 0 is the **plate**, before the effect fires; the effect fires on step 1; it holds wherever
  `window.__fxStep` asks; `window.__fx` says where it stands.
- **What it fires:** the cue the director would make (`Staged::cue`), handed to the same bursts
  (`fx/burst.rs`); a missile is a shot flown across the card at the ability's own speed and width,
  drawn by the live pool (`fx/live.rs`).
- **Names:** `<champion>-<q|w|e|r>-<part>` (`cast`, `launch`, `impact`, `blast`, `windup`, `dash`,
  `buff`, `ult`), `<champion>-<slot>-missile`, `<champion>-auto-<launch|impact|missile>`,
  `blink-dash`, `tag-*`, `turret-*`, `status-<stun|root|…>`, `shield`, `heal`, `fizzle`,
  `death-<champion|minion|turret|inhibitor|nexus>`, `respawn`, `levelup`, `relic`, `relic-beam`.

## The numbers

Every number is measured per card against the plate, on sRGB 0–255 Rec.601 luma.

| number | what it says | read it as |
|---|---|---|
| `add` | mean light added over the card | glow: sparks, a flash, fire. Peaks in the first frames of a hit |
| `sub` | mean light taken away | smoke, dust, a scorch. Mostly on the light card |
| `cover` | % of the card that changed at all | how much of the frame the effect owns |
| `peak` | 99.9th-percentile luma of what changed | 255 = something hit white |
| `clip` | % of the changed pixels at 250+ | a flash may clip for a frame; a cloud never should |
| `bbox`, `c` | where it changed, 0–1 of the card; its centre | spread and drift; `travel` is how far `c` moved |
| `envelope` | `peak`/`peak_t`, `fall_t` (half), `life_t` (last seen), `area_max`, `travel` | the effect's shape in time |
| `vs_prev` | % of each frame that moved since the last run | a change should move only the frames it means to |
| `plate.ok` | the cards read ~26 and ~183 | if not, the stage's light changed and nothing else is comparable |

The numbers keep a floor under the eye; they never replace it. Look at the strip.

## Make or tune one

1. **Write the brief before touching anything** (3–5 lines): the event's **envelope** (an instant
   or sustained, how long), its **size** in metres, its **colour and temperature** over life (the
   family's palette: `el_client_core::fx::look`), what must read on **each card**, the
   **reference** if any (Riot's rules in `docs/FX.md`: the part that does the gameplay is the
   brightest and sharpest; a missile shows its direction standing still).
2. **Find the recipe.** `el-client/src/fx/burst.rs`, `recipe()`: a match on `What`, each arm the
   bursts a cue throws (`spray`, `one`; the shapes `shape::*`; a family's `sparks`). The flipbook
   shapes (a puff, a fireball, a flash of petals) play `web/assets/fx/cloud_{a,b}.png`, baked by
   `art/tools/flipbooks.py`; their lighting, erosion and ramp are `shaders/fx_burst.wgsl`. A
   missile's head and trail are `fx/live.rs` and `shaders/fx_live.wgsl`.
3. **Stage it.** A new part of an ability is in the catalogue already (`stage::list` follows the
   look); something new to the director needs a `Spec` in `el_client_core::fx::stage` and a line
   in `Staged::cue` and `Staged::lift`.
4. **Shoot, read, judge** against the brief, or send the `fx-judge` agent
   (`.claude/agents/fx-judge.md`) for a read that is not anchored on yours.
5. **Tune with knobs**, one idea at a time: `--knobs 'name=value;…'` (`scripts/fx.sh knobs`:
   `count`, `size`, `life`, `speed`, `bright`, `smoke`, the shader's `hot`, `night`, `puff`,
   `flash`, and a part's own: `impact.star`, `blast.flash`, `buff.glow`, `missile.core`…). Every
   knob is a multiplier, 1 in a match.
6. **Write it in.** Put the numbers into `recipe()` (or the shader), rebuild, and shoot once more
   without knobs. `diff.png` must show only the frames you meant to change, and `vs_prev` must be
   ~0% against the last knob run.
7. **Check what you can't see on one roll:** two more `--seed`s for anything random.
8. **Then:** `cargo test -p el-client-core --release stage`, `cargo clippy -p el-client --target
   wasm32-unknown-unknown -- -D warnings`, and `docs/FX.md` step 7's "What landed" gets a line
   with the numbers, before and after.

## Judging (Riot's rules, `docs/FX.md`)

- **Every burst has a role:** what does the gameplay, what shows direction, what's residue. One
  you can't name a role for goes.
- **One event drives every burst.** Their onsets line up: a hit peaks in the first two or three
  frames (`peak_t` ≤ 0.05 s on the dark card), its residue comes after, and nothing starts late by
  accident. A hit whose dust or foam peaks at half a second outweighs the strike (Brakka's Q,
  Talmar's Q did).
- **The part that does the gameplay is the sharpest thing**: a hit's flash, a missile's head, a
  blast's ring. A wide soft core in a halo as wide as its quad is a pale blob (Vessa's W was).
- **It reads with bloom off.** WebGL2 has none; the high tier on WebGPU adds it later.
- **Smoke reads on the light card, light on the dark one.** Dust the colour of the card is
  invisible there; the numbers say so (`sub` ≈ 0).
- **Clip is for an instant.** `clip` over ~5% past the first frames is a blown-out blob; lower
  `bright`, `flash` or the size.
- **Life matches the brief.** `life_t` and `gone`: nothing hangs past the brief or pops out.
- **The family's hue holds.** A ramp that goes white early makes a cloud of every fireball.
- **Motion from the strip, never one frame.** A good still can hide a pop or a stall.
- **Three seeds before calling anything random good.**
- **Budget.** `live` (light/smoke slots) against the pools (72/24 on the high tier, 40/12 on
  the low; 48 particles a burst, the hit's core and flash first): one blow should use a slot or
  two; a fight is dozens.

## Rules

- `e2e/fxstrip.mjs` is Gates' (MIT), vendored; fix it in Gates, then copy.
- The sheet's cut is one set of numbers in the baker, the shader and `stage::sheet`; a test holds
  them together. A change to a sheet is a change to every effect that plays it: shoot the strips
  that use it.
- Numbers live in the recipes, never in docs; `docs/FX.md` says what changed and what it measured.

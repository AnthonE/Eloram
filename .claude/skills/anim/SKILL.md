---
name: anim
description: Make or tune how a champion (or minion) moves in Eloram - its clips, the timing's shape (the wind-up's snap, the hold on contact, the follow-through), the motion on top (turning, leaning, recoil, squash and stretch), its flinches, the swing's arc and the casting hand's light - on the anim stage - stage one champion doing one thing in front of a dark and a light card, read its filmstrip, retune the motion with knobs without a rebuild, compare against the last run, and write the numbers that work in. Use when asked to add, fix, tune, compare or review an animation, a champion's feel, its attacks or its casts.
---

# Animation in Eloram

How a champion moves is judged as it moves, never from a still, and never from a match in headless
Chromium (SwiftShader draws a frame every second or so: a swing falls between frames). The loop is
the fx stage's:
1. stage one champion doing one thing;
2. read its **strip**;
3. change one thing;
4. read the strip again and its **diff** against the last run.

The stage is `el-client/src/animstage.rs` on the fx stage's clock; its catalogue and scripts are
`el_client_core::anim`, the timing and the layers `el_client_core::pose`, the motion
`el_client_core::motion` (all tested natively); the shooter is `e2e/fxstrip.mjs`, the same as the
fx stage's.

```bash
scripts/anim.sh list [filter]                     # every champion's every act (260)
scripts/anim.sh knobs                             # the motion's knobs, with the game's values
scripts/anim.sh vire-attack                       # → e2e/artifacts/anim/vire-attack/{strip.png,numbers.json,diff.png}
scripts/anim.sh vire-attack --knobs 'snap=0.3;hold_melee=0.1'   # retune, no rebuild
scripts/anim.sh vire-attack --steps 0,14,18,22,26,30            # exactly these steps (around a strike)
scripts/anim.sh --no-build …                      # the page as it was last built
node art/tools/champions.mjs --sheet web/assets/models/champions/vire.glb out.png   # every clip as stick figures, no browser
```

## What's what

- **The clips** (`el_client_core::pose::Clip`): twenty from Meshy's library, retargeted from one
  rig onto every champion; eighteen from Quaternius's CC0 Universal Animation Library
  (`art/champions.json`, `ual`), retargeted from its T-pose onto each champion's A-pose
  (`art/tools/retarget.mjs`) and put on the models by `node art/tools/champions.mjs --ual`. Each
  clip has a length and a **strike** (when it lands); the library's are where the weapon's tip or
  the hand moves fastest, as `--ual` measures them.
- **A champion's kit** (`pose::kit`): its stance (`IDLES`: a guard, a shield, a crouch, a light
  held up, folded arms), two attacks it alternates (`ATTACKS`), a clip a cast by how the ability
  reaches, its dash. A new clip in a kit: `cargo test -p el-client-core pose -- --ignored
  write_clips`, then `--ual` (a library clip) or the full build (Meshy's).
- **The timing's shape** (`pose`, `Warp`): the wind-up eased in (`snap`), held on the strike
  (`hold_melee`, `hold_ranged`, `hold_cast`), followed through (`follow`). The strike always lands
  as the server's windup ends: that never moves.
- **The layers** (`figure.rs`): the upper body and the legs are separate nodes (Bevy's masks), so
  a cast walked through keeps the legs running (`Pose::legs`) and a flinch takes the upper body
  (`Pose::flinch`, `flinch`, a reel past `heavy` of its health).
- **The motion** (`motion`): the turn's time constant (`turn`), the lean (`lean_run`,
  `lean_accel`, `bank`, `lean_max`), the recoil (`recoil`, `recoil_share`), squash and stretch
  (`squash_land`, `squash_strike`, `stretch_dash`, `squash_fall`).
- **What it draws** (`trails.rs`): a swing's arc from the weapon's tip (`<prop>.tip` in the model),
  the casting hand's light; and what it kicks up (`fx/burst.rs`, `kick`): a step's dust, a landing's
  ring, a dash's puff.

## The stage

- **One champion, twice**: in profile, facing the screen's right, over the **dark card** (the
  frame's left half); three-quarters toward the camera over the **light card** (the right half).
  Both do the same thing at once.
- **Acts** (`anim::Act`): `stand`, `run` (off, a run at its own pace, a stop), `attack` (three at
  level 1, alternating), `q` `w` `e` `r` (cast standing), `walkcast` (its Q on the run), `hit` (a
  light hit from in front, a heavy one from its left), `turn` (an about-face, back, a weaving run),
  `land`, `fall`, `win`. Names are `<champion>-<act>`.
- **The clock** is the fx stage's: step 0 the plate (the figure standing), the script starts on
  step 1, a step 1/60 s, held where `window.__fxStep` asks. `window.__fx.clips` says what each
  figure plays.
- **Script times** (`anim::START` = 0.25 s): an attack's first wind-up starts at step 16 and
  strikes after its windup (`anim::attack_timing`); a cast starts at step 16.

## Judging (Riot's, `docs/CHAMPIONS.md`)

- **The silhouette first.** At 60 px tall the pose must read: the weapon (drawn 25% bigger than
  made, `figure::HELD`) and the limbs clear of the body at the key poses.
- **Anticipation, then a fast strike, then the hold.** The frames before the strike should show
  the wind-up; the strike itself one or two frames; the contact held a few. A strike smeared over
  many frames is floaty (raise `snap`); one that jumps from rest to contact has no anticipation
  (lower it).
- **The strike lands on the windup's end.** Count the steps: an attack's first strike is at step
  `16 + windup * 60`.
- **Two attacks differ.** The second swing isn't the first again.
- **The body stays in its hitbox.** Hips rise and fall; nothing travels across the card.
- **Turns are quick and never overshoot.** An about-face is done in a tenth of a second or so.
- **A hit reads**: a flash, the upper body thrown back from where it came, back in a fifth of a
  second; a heavy one more.
- **A cast walked through:** the legs keep running, the arms cast; nothing in the legs hitches as
  the cast ends.
- **Every champion of a kit shares the numbers.** A knob is the game's for all twenty: shoot two or
  three different bodies (a big one, a small one, a ranged one) before writing it in.

## Tune one

1. Write the brief (3–5 lines): which act, what should read at the strike, how long the hold, how
   far the lean.
2. Shoot it, read it, or send the `anim-judge` agent (`.claude/agents/anim-judge.md`).
3. Retune with `--knobs`, one idea at a time.
4. Write it in: `Feel::GAME` in `el_client_core::motion` (or a clip's strike in `pose::Clip::info`,
   a kit in `pose`), rebuild, shoot once more without knobs; the diff moves only what you meant.
5. Then `cargo test -p el-client-core --release` and `cargo clippy -p el-client --target
   wasm32-unknown-unknown -- -D warnings`, and `docs/CHAMPIONS.md` gets a line with what changed.

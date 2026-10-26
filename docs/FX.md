# Effects and sound

The plan for what an ability looks and sounds like: the cast, the windup, the missile, the blast,
the field, the hit, the status after it, and every other thing in a match that should be seen or
heard (autos, turrets, minions, structures falling, relics, levels, augments, the HUD). It comes
before champion animation because all three hang off the same moments, and the sim already says
when those are, in ticks. Animation will be timed to the windup this builds on, not the reverse.

Riot's posts are the source on what makes League's effects read (sources at the end). Nothing of
theirs is traced, sampled or named (`CLAUDE.md`, "Every name is the game's own").

## Where it stands

The sim has an effect system and the client draws almost none of it.

- **The sim** (`el-sim/src/content/mod.rs`): an ability is how it reaches (`Line`, `Homing`,
  `Strike`, `Area`, `Zone`, `Dash`, `Blink`, `OnSelf`) and a short list of ops (damage, heal,
  shield, stun, root, silence, airborne, slow, knockback, pull, buff, reveal). Twenty champions
  times four abilities, Blink and Tag, autos, turrets and relics are all built from those.
- **The client** draws missiles as spheres in team colour, an area that lands or a ping as a
  fading disc (`view.rs`, `draw_flashes`), and the cast indicators. There is no sound: Bevy is
  built without `bevy_audio`, and nothing plays one.
- **The wire** already carries much of what effects need:
  - `Cast { unit, slot, at }`, so the ability (the champion's def by its slot), and where it went;
  - a unit's `cast` field (the slot it's winding up), so a windup is seen while it lasts;
  - `MissileSpawn { source, .. }`, the ability a missile is (a closed form, drawn on the predicted
    clock);
  - `Attack`, `Blink`, `Death`, `Respawn`, `LevelUp`, `Relic`, `Augment`, `Outcome`;
  - the status flags (`el_proto::unit::flags`: stunned, rooted, silenced, airborne, slowed,
    dashing, attacking, shielded, revealed, immune).
- **What it doesn't carry:**
  - `Area` names no source: Orrin's ash landing and Torek's shell are the same event.
  - Zones aren't told when they're made. A blast that lands late can't be telegraphed, and an enemy
    who can't see the caster gets no warning at all; a field that pulses shows only as pulses.
  - `Damage` and `Heal` name no kind and no ability, so a hit can't look like what hit it, and a
    number can't say physical, magic or true.
  - A buff (a war cry, a rally, a glow) has no flag.
- **Two faults found on the way**, both fixed in step 1:
  - `Source::code` packs the champion into four bits, so champions 16 to 19 come out as 0 to 3
    (and `Source::Ability` with champion 16 has the same code as champion 0's).
  - A zone that follows its owner pulses at the owner's position, and `Area` goes to any team that
    can see where it lands: a champion in brush, unseen, is given away by the ring round it.

## What Riot says

From Riot's dev posts and their VFX artists' write-ups:

1. **Gameplay first, then noise, then theme.** In their VFX updates the order is fixed:
   gameplay (hitbox indicators that are wrong or missing, unclear spells) above readability and
   visual noise, above theme. "Readability will always prevail over thematic resonance", and
   correcting hitbox indicators comes first.
2. **Attention matches importance.** "The amount of attention an ability grabs should match its
   level of importance", judged by four things: damage, crowd control, how dodgeable it is, and how
   much it changes how you play. "Visual impact should represent gameplay impact." Big spells
   "can and should be noisy, to a degree"; the rest stay quiet, above all in teamfights.
3. **Importance is rated for each viewer.** Riot's artists rate a spell 1 to 10 for the caster,
   their allies and their enemies, and when the ratings differ the effect differs. A range circle
   that tells allies nothing is shown only to the caster and the units it holds. "If everything is
   important, nothing is important."
4. **An area of focus.** The part of the effect that does the gameplay (a missile's head, a
   blast's edge) has the most value contrast, the most saturation and the sharpest detail; the
   rest (a trail, embers) is secondary, there for theme, and never competes with it.
5. **A missile shows its direction even standing still**: a tail blurred behind it.
6. **A shared language for statuses.** A root is a ring on the ground and a cylinder rising from
   it, and every root looks like that; a knockup is strongly vertical; a heal is green and nothing
   threatening; a shield has lines panning down it; stun, silence and poison use the tropes players
   already know.
7. **Colour blindness.** The common kind is red against green: check a red effect against the
   ground it lands on.
8. **Sound is a palette.** Each champion's sound starts as a palette of building blocks (Foley,
   processed, pitched, run through synthesis), and champions of one place share sonic traits
   without sounding the same. Every sound balances gameplay clarity against the character.
9. **Dangerous spells warn by ear.** The examples Riot gives of clear spells have an audio cue
   before they land (a windup on a stun, a cue before a big hit), and an ultimate has an immediately
   recognisable callout.
10. **Who hears what.** Voice lines are aimed: a recall line is the caster's alone, a kill line the
    caster's and the enemy team's.
11. **The mix has a ladder.** A Riot sound designer describes a priority ladder for what leads when
    many things fire, headroom kept for gameplay cues, light ducking of the other buses, and, when
    a fight spikes, a mix that rebalances what leads rather than getting louder.
12. **Where effects draw.** League draws its particles after the world and before post-processing,
    then health bars, then the HUD; in the frame Riot walks through, 90,000 of 200,000 triangles
    are particles. Effects are a large share of a frame and need a budget.

What this means here, as rules (the readability rules of step 5):
- Every telegraph and missile is drawn from the wire's own radius, width and range, so the art
  can't lie about the hitbox. A test holds it.
- Each ability in the look table carries three ratings (self, allies, enemies), and the ratings
  set size, brightness, sound priority and whether a part draws at all for that viewer.
- Enemy effects are drawn louder than allied ones, and an enemy's telegraph has a red-orange edge
  that reads against the Causeway's blue stone; allies' are cool. Every pair is checked in a
  red-green simulation.
- Statuses look and sound the same whoever caused them (the flag decides, not the champion).

## The work, in order

Each step lands on its own with `scripts/ci.sh` green, and the game stays playable if an asset
doesn't load: today's spheres and discs are the fallback.

### 1. The wire (protocol 4)

- `Source` gets a code with room for the whole pool (the champion in five bits, the slot in three),
  and the platform's laser one of its own.
- `Area` carries its source.
- A new reliable `ZoneSpawn`: a zone as a closed form (where, how wide, its source, its team, when
  it first fires, its period, when it ends, whether it follows its owner), told when the team
  first sees its ground, as missiles are; and `ZoneEnd` when it's gone. A zone from a caster the
  team can't see names no caster. A zone that follows its owner is told only while the owner is
  seen, and its pulses the same, which closes the brush leak above.
- `Damage` carries its kind (physical, magic, true) and the source code; `Heal` and a new `Shield`
  event carry the source too.
- Two flags: empowered (a buff to its damage, attack speed or resists) and hasted (a buff to its
  move speed), so a war cry or a rally shows on whoever has it.
- The vision-leak oracle decodes the new events and stays at zero; `netcode.rs` reports the bytes
  a client takes before and after.

This changes what clients are told, never what the match does: `CONTENT_VERSION` holds; the
protocol bumps (records name it, and `publish-web.sh` refuses a page that speaks another).

A missile or zone from an unseen caster still carries its source, as now: what it is gives away
who threw it, as it does in League, never where they stand.

### 2. The effects runtime (`el-client/src/fx/`)

- **Cues.** One system reads the session's events, the unit flags and the missile and zone tables,
  and turns each into cues: `(look part, where, when, who for)`. Every `WireEvent` arm and every
  flag has its line, with no wildcard arm, as replication has.
- **Timing.**
  - The own champion's cast cues start from the prediction's own `Cast` (it steps the server's
    code, so the event is there a round trip early), matched to the server's by unit, slot and
    tick; one the server never confirms fades out.
  - Everyone else's play at the interpolated clock they're drawn at, not when the datagram came.
  - Missiles and zones are closed forms on the predicted clock, as missiles are now.
- **Particles as closed forms.** A burst is `(origin, t0, seed, kind)` in an instance buffer, and
  the vertex shader moves each quad from its age, as the snow does: no system writes a transform a
  frame, and it runs the same on WebGL2 and WebGPU. Bevy's usual GPU particles want compute
  shaders, which WebGL2 doesn't have. Everything is pooled at startup, as the unit slots are.
- **The kit**, what every look is built from:
  - *burst*: sparks, embers, shards, motes, ballistic or radial, additive or alpha;
  - *trail*: a ribbon behind a missile, from the missile's own closed form (the tail Riot asks for);
  - *telegraph*: a ground ring or line at the hitbox's exact size, filling toward the moment it
    lands, team-edged;
  - *field*: a lasting zone, its edge bright and its body quiet, pulsing on its period;
  - *beam*: tethers, the relic's beam, a skyfall's column;
  - *shock*: an expanding ring or dome;
  - *flipbook*: a camera-facing sprite sheet (the braziers' flame already is one);
  - *light*: a short point light (high tier only);
  - *status*: what each flag shows on a unit (rings and a rising cylinder for a root, a vertical
    lift for airborne, stars for a stun, a hushed mark for silence, a downward shimmer for a
    shield, chill or drag at the feet for a slow, a glint for immune);
  - *hit*: the model flashes for a frame or two, and the own champion's camera shakes a little on
    a heavy hit or crowd control.
- **Where it draws**: after the scenery and units, before bloom, under the health bars and HUD.
  Effects in the fog dim as the scenery does (`fog.wgsl`); nothing the server didn't send can draw.
- **The tiers**: `low` halves particle counts, drops lights and soft edges; `high` adds the
  lights and bloom.

### 3. Sound (`el-client/src/audio.rs`)

- **Web Audio directly**, through `web-sys`, not `bevy_audio`: the browser decodes the files, so
  no decoder goes into the wasm (the JPEG decoder alone was a megabyte); it schedules a sound to
  the sample, and its voices are cheap. One `AudioContext`, resumed on the first click (the queue
  button), as browsers ask.
- **Buses**: effects, voice, the HUD, ambience, music, under a master with a limiter; a slider each
  on the page.
- **The ladder**, which sound leads when many fire: the own champion's casts and the crowd control
  landing on it; then enemy ultimates and windup warnings; then enemy casts and hits; then allies';
  then minions, turrets and the world. A cue's priority is its look's rating for the listener
  (step 2), so an enemy's stun is louder than the same stun of an ally's.
- **Voices**: a cap per cue (an auto has three going at most) and a cap overall (32); past it the
  lowest of the ladder is cut, never the newest of the top. When a fight crowds the mix the lower
  rungs duck a few decibels, the top doesn't get louder.
- **Space**: panned by where the sound is across the camera's view, quieter with distance from the
  view's centre, and nothing for what's off the edge of the screen unless it's on the ladder's top
  two rungs.
- **Variation**: three or four takes of anything frequent (autos, hits, minion swings, footfalls),
  and a few cents of pitch drawn each play, so a fight doesn't machine-gun.
- **Fetched by match**: the shared set (statuses, autos, minions, turrets, the HUD) with the page;
  each champion's palette once champion select deals it, so it's there when the match starts.
- **Nothing to leak**: a sound plays only for what this team was sent, so it can tell no more than
  the screen.

### 4. Practice

Authoring eighty abilities needs a place to cast them. `el-server --practice` (and `?practice=1`
on the page): one seat, any champion (`{t:"practice_champ"}` from the console or a test), every rank
open, no cooldowns, and target dummies on both teams (a champion's stats, standing, walking a loop,
or shielded) at the Causeway's middle. It's a match like any other, with its own content flag, so
nothing about it touches a real one.

### 5. The looks

The look table (`el-client/src/fx/looks.rs`) has a line for every ability of every champion, Blink,
Tag, each role's auto, the turret's shot, the relic, and each augment's trigger: its parts, its
palette, its ratings and its sounds. It mirrors the sim's: a fixed kit, and data saying how each
ability uses it.

Families share colour, material and sound, as Riot's regions share sonic traits:

| Family | Champions | Look | Sound |
|---|---|---|---|
| Ember | Orrin, Mira, Brun | orange-gold fire, ash, sparks | crackle, roar, hiss, the forge |
| Frost | Nimra | pale blue glass, rime, shards | glass chime, crack, crunch |
| Tide | Talmar | deep green-blue water, foam | surge, splash, undertow |
| Wind | Sefa | white arcs, leaves, streaks | whoosh, whistle, gust |
| Dusk | Seren, Sable | violet and starlight, shade | low hum, shimmer, a hush |
| Growth | Ulla | green motes, petals, thorns | rustle, creak, bloom |
| Stone | Brakka, Gorum, Torek | grit, dust, cracks, iron | thud, rumble, grind, report |
| Bronze | Hollis, Liora, Oda, Pell | warm light, brass, banners | bells, chimes, horns |
| Steel | Vessa, Marret, Kesh, Vire | sharp white, motion streaks | twang, ring, shot, slice |

Order: the shared parts first (statuses, autos, minions, turrets, deaths, levels, the relic, the
HUD), then a champion at a time through the families, each finished (look, sounds, ratings,
screenshots) before the next.

### 6. The mix and the look of a fight

A ten-champion fight in practice (dummies with abilities cast on a loop) and in a bot match:
the readability rules held, the frame budget held, the mix's ladder heard. Writes the frame-time
numbers on a real GPU into `ROADMAP.md`, which has none yet.

## Sound: what it's made of

- **Sources**: CC0 recordings (Kenney's audio sets, Freesound's CC0 pool, our own Foley), never a
  file whose licence isn't in `art/LICENSES.md`. Each champion's palette is built from them by
  `art/tools/sounds.py`: layered, pitched, filtered, enveloped, rendered to its takes, as Riot's
  palettes are made, from building blocks.
- **Format**: Opus in WebM for the browsers that take it, and AAC for the rest if Safari needs it
  (checked before choosing), mono at 48 kHz, normalised to a loudness by bus.
- **Budget**: about 300 files, under 6 MiB for a match's set (the shared set and ten palettes).
- **Not in this pass**: an announcer and champions' voices, and music. Both get buses now, so they
  slot in later; each is its own pass.

## How we know it's done

- **Coverage**: a native test (`el-client-core`) that every champion's every slot, every `Source`,
  every `WireEvent` and every flag has a look and a sound, matched exhaustively.
- **Hitboxes**: a test that each telegraph, field and missile is drawn at the size the wire gives,
  and the vision-leak oracle at zero with the new events in it.
- **Pictures**: `scripts/e2e.sh fx` plays practice, casts each ability of each champion at a dummy,
  and writes a contact sheet to `e2e/artifacts/` (windup, flight, impact, status) for the PR.
- **Sound**: `window.__el.audio` lists the cues played; the `fx` suite checks every cast played its
  sound, and a fight never went past the voice cap.
- **Frame**: on a real GPU, a ten-way fight holds 60 fps at 1080p on `high`, and the effects'
  share of the frame is measured.
- **Nothing else moves**: the golden hashes hold, `CONTENT_VERSION` holds, no allocation in a tick,
  `ci.sh` green.

## What landed

**Step 1, the wire (protocol 4).**
- `Source::code` has a champion in five bits and a slot in three (`Source::Fountain` is 2), and a
  test that every source of the pool comes back as itself with a code of its own. The record's
  hash takes the new code, and the golden hashes held (so no ability's missile is in flight at
  either tick they're taken): what the match does didn't change.
- The sim's `Damage` carries its kind and its source, `Heal` its source, and a new `Shield`;
  `apply_ops` and `deal` take the `Source` they act for where they took a flag. `Area` names its
  zone, source and owner, and whether it follows them; a zone has a generation.
- On the wire: `ZoneSpawn` and `ZoneEnd` (reliable), `Shield` (not), `Damage` with its kind and
  source, `Heal` and `Area` with theirs, and two flags, empowered and hasted (`FLAG_BITS` 12). A
  zone's next pulse and end go as ticks either side of the snapshot's.
- Replication tells a zone when the team first sees its ground, or, for one that follows its owner,
  the owner; and ends it when it ends or that owner goes out of sight. A following zone's pulse
  goes only where its owner is seen. `el_client_core::zones` keeps what was told: where, when it
  fires next, how far its telegraph has filled.
- The leak oracle checks the new events: 0 leaks over 14,734 events in five minutes, 62 zones told
  (31 an enemy's, 16 following their owner). Put back the old rule for a pulse and it finds two in
  the same match (an enemy's following zone's pulse, its owner unseen).
- Down a client over 100 ms and 5% loss (`netcode.rs`), 3.7 to 3.8 KB/s, as before: the new
  fields cost less than the noise between runs.

## Sources

- Riot Games, [Clarity in League](https://www.leagueoflegends.com/en-us/news/dev/clarity-in-league/).
- Riot Games, [/dev: Behind the scenes of VFX updates](https://www.leagueoflegends.com/en-pl/news/dev/dev-behind-the-scenes-of-vfx-updates/).
- Riot Games, [/dev: League's VFX style guide](https://nexus.leagueoflegends.com/en-us/2017/10/dev-leagues-vfx-style-guide/) (2017).
- Oliver "Beardilocks", Riot VFX, [Anatomy of a League of Legends missile, part 1: areas of focus](https://realtimevfx.com/t/anatomy-of-a-league-of-legends-missile-part-1-creating-areas-of-focus/11614)
  and [part 2: what are we communicating](https://realtimevfx.com/t/anatomy-of-a-league-of-legends-missile-part-2-what-are-we-communicating-with-our-vfx/13062).
- Riot Games, [The stories behind League's SFX](https://nexus.leagueoflegends.com/en-us/2018/07/the-stories-behind-leagues-sfx) (2018).
- Riot Games, [Ask Riot: let's talk clarity](https://www.leagueoflegends.com/en-gb/news/dev/ask-riot-let-s-talk-clarity/).
- Riot Games, [League Frequencies](https://riotgames.com/en/news/league-frequencies-audio-development-video-series), the audio team's video series.
- Player One, [How Riot sound designer Sandy Zhou designs adaptive audio for clarity](https://www.player.one/teamfights-exploration-how-riot-games-sound-designer-sandy-zhou-designs-adaptive-environmental-162190) (an interview, not Riot's own post).
- Tony Albrecht, Riot Games, [A trip down the LoL graphics pipeline](https://www.riotgames.com/en/news/trip-down-lol-graphics-pipeline).

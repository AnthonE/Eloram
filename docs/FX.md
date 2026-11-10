# Effects and sound

The plan for what an ability looks and sounds like: the cast, the windup, the missile, the blast,
the field, the hit, the status after it, and every other thing in a match that should be seen or
heard (autos, turrets, minions, structures falling, relics, levels, augments, the HUD). It comes
before champion animation because all three hang off the same moments, and the sim already says
when those are, in ticks. Animation will be timed to the windup this builds on, not the reverse.

Riot's posts are the source on what makes League's effects read (sources at the end). Nothing of
theirs is traced, sampled or named (`CLAUDE.md`, "Every name is the game's own").

## Where it started

As it was before this pass ("What landed", below, is where it stands now).

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

### 7. The stage, and the flipbooks

Step 5's hand-tuning needs a place where one effect can be looked at alone, as it moves, and
measured, so a change can be seen to do what it meant and nothing else. Gates built that loop
for its effects (`AnthonE/Gates`, `crates/fx/src/stage.rs`, `ci/fxstrip.mjs`, the `fx` skill)
and oneshot runs the same loop against its browser page; Eloram takes the contract and the
shooter as they are, and keeps its own particles, which are closed forms moved in the shader
(step 2), not Gates' pool.

- **The stage** (`?fxstage=<name>`, `el-client/src/stage.rs`; the catalogue, the clock and the
  knobs are `el_client_core::fx::stage`, tested natively): one effect, fired twice at once, in
  front of a dark card (the frame's left half: light reads here) and a light card (the right:
  smoke, dust and a scorch read here), laid on the Causeway's middle with everything else hidden,
  the camera close over them at the effect's own height. The picture's clock steps exactly 1/60 s
  a frame and holds where the shooter asks (`window.__fxStep`; `window.__fx` says where the page
  stands): step 0 is the plate, the effect fires on step 1. No server: the stage makes the cue
  the director would (`Staged::cue`) and hands it to the same bursts.
- **The strip** (`e2e/fxstrip.mjs`, Gates' own, vendored; `scripts/fx.sh <name>`): every frame
  captioned, the brightest large, an envelope of how much each card changed over time, and the
  numbers per card against the plate (light added, light taken, cover, peak, clip, where), a diff
  against the last run, all in `e2e/artifacts/fx/<name>/`. Headless Chromium draws in software,
  but a clock that holds for every frame doesn't care: a strip is right at any frame rate.
- **Missiles** (`vessa-w-missile`, `orrin-auto-missile`, `turret-missile`): a shot flown from
  left to right across each card at the ability's own speed and width, drawn by the live pool
  (its head and trail) at the stage's own tick, where a match's client would give it missiles.
- **Knobs** (`scripts/fx.sh <name> --knobs 'impact.star=1.5;size=0.8'`; `scripts/fx.sh knobs`):
  named multipliers on the recipes' numbers, read where a burst is made and in the shader, so a
  look is retuned without the four-minute wasm rebuild, and the numbers that work are then
  written into the recipe. In a match every knob is 1.
- **Flipbooks** (`art/tools/flipbooks.py`, `web/assets/fx/cloud_{a,b}.png`): smoke, a fireball
  and a flash of petals baked from a small fluid simulation (Stam's stable fluids with a
  multigrid pressure solve, after oneshot's baker) into one 1024² sheet pair, each frame lit from
  six sides, so the shader lights a puff from wherever the moon stands in the sprite's own frame,
  erodes it from its thin parts as it ages, and adds its heat through the family's ramp. A puff,
  a blast's fireball (fire burning to soot where fire or stone strikes; the heat alone, an energy
  burst, for the rest) and a hit's flash play them; the bursts' other shapes are drawn as before.
  The sheet's cut is one set of numbers in the baker, the shader and `stage::sheet`, held together
  by a test.

## Sound: what it's made of

- **Sources**: CC0 recordings (Kenney's audio sets, Freesound's CC0 pool, our own Foley), never a
  file whose licence isn't in `art/LICENSES.md`. Each champion's palette is built from them by
  `art/tools/sounds.py`: layered, pitched, filtered, enveloped, rendered to its takes, as Riot's
  palettes are made, from building blocks.
- **Format**: Opus in WebM for the browsers that take it, and AAC for the rest if Safari needs it
  (checked before choosing), mono at 48 kHz, normalised to a loudness by bus.
- **Budget**: about 300 files, under 6 MiB for a match's set (the shared set and ten palettes).
- **The voices** (the announcer and the champions) came in their own pass: "The voices, and the
  rewards", below. The music is still to come; it has its bus and its states.

## The voices, and the rewards

The second sound pass: what makes a MOBA's sound more than effects (the announcer, the champions'
voices, the rewards a player hears alone), as Riot does it, built with placeholder voices so the
whole thing plays now and every line is ready for a recording.

### What Riot does

**The announcer** is information first, and it's aimed. Its lines are said from the listener's
side: "you have slain an enemy", "an enemy has been slain" (your team scored), "an ally has been
slain", "you have been slain". Multikills and sprees come twice over, an ally's and an enemy's
("double kill", "enemy double kill"; "killing spree", "enemy killing spree"); then a shutdown (a
spree ended), an ace, an execution (a champion killed by no champion), each structure by whose it
was, an inhibitor respawning soon and respawned, each side's, and the result. A map has its own
welcome and its own voice (the Howling Abyss), and an event announcer changes the voice, never the
structure (the wiki's announcer pages). What's in fog is fog: an execution there was announced to
both teams until patch 13.19 fixed it.

**A champion's voice is a script of moments**, each line its own category (the League wiki's
"Quote" and its manual of style): a pick and a ban in champion select; the match's first move;
moves (at least 25 lines) and long moves (a click over 2,000 units away; 15 more, the lore);
attacks on a champion (25); the first encounter with a champion (capped at five a game);
taunts, jokes, laughs, dances; ability casts, their efforts, and the ultimate; kills, assists,
sprees and multikills; ranking up an ability; the death and the respawn; the result. Each
category is throttled, by chance and cooldown, and Riot tunes it champion by champion (patch 9.13
made three champions' ability lines rarer): a move line no more than every 15 s, a basic attack's
30 to 50% of the time, an ability's 50%, a respawn's 75% (datamined, on Vi's and Mel's pages).
Lines that answer a player's own clicks are that player's to hear; the ultimate is everyone's who
sees it (Riot: an ultimate has "an immediately recognisable callout").

**How a voice is made** (Riot's "The Voice of the Visionary", 2023): writers draw up a casting
sheet with "roster comparisons to flag vocal overlap"; a script goes scratch, audition,
exploratory, final; the whole script is recorded in one four-hour session, several takes a line;
a VO designer builds "voice filters" and finds "impactful game moments to trigger lines from";
a champion's efforts are grouped by spell, kept "distinct by their energy, length and their
beginning consonant". A processed voice is built in layers ("The Frequencies of Folklore", 2020,
Spirit Blossom Thresh): one for intelligibility, a muffled deeper copy, low end "felt, more than
heard", a reversed reverb. Lines can hang on conditions (a line only beside a certain champion:
2017's Contextual Action Component).

**The juice.** Riot's kill and takedown sounds "play for your ears only" and "escalate" with a
multikill (patch 25.11). Attention matches importance: "the most important thing at any given
moment (like a major ultimate or CC) should draw the most attention" ("Clarity in League", 2021),
and "iconic doesn't mean louder. It means recognizable" (Sandy Zhou). Abilities cast out of
sight are "no longer visible or audible in Fog of War" (patch 10.2); the few global ones are heard
by all and shown on screen beside the kill callouts for those who can't hear them (13.19). Spells
are panned by where they are on the screen (2024).

### What makes it juice, as rules here

1. **Sound is information before it's impact.** The announcer tells everyone what happened from
   where they stand; an ultimate's callout warns as its sound does; nothing is heard that the
   team wasn't told (the voices ride the same events as the effects, and the fog is the server's).
2. **The player's reward is theirs alone, and it climbs.** A coin on every last hit; a kill's
   sound climbing with the kills close together (one bell note up for a kill, five and a chord
   for all five); a softer one for an assist. Never placed, never heard by anyone else.
3. **The champion answers the player.** A move, a long walk, an attack on a champion, the first
   enemy seen, the ultimate ranked up, the return from death: each can get a line, throttled by
   Riot's numbers so it's character, not noise.
4. **One voice a champion, three champions at most, the player's always.** A champion never
   talks over itself, but its callout, its kill and its fall cut in; the player's own lines make
   room; the announcer is over them all, and they sit a little under it.
5. **The announcer is seen as well as heard.** The line said is a banner, in the colour of whose
   news it is, naming who killed whom; with no recording it's still the banner.

### What was built

- `el_client_core::fx::voice` (the announcer): Riot's structure in the game's own words, each
  line's words in `says` (what's recorded, and the banner's text), 44 lines: first blood; two to
  five kills close together, ours and theirs ("Double kill!", "Enemy double kill."); three to eight
  and more without falling, ours and theirs ("On a run!", "An enemy is on a run.", up to "Beyond
  legend!"); "The run is broken!"; "Felled by the Causeway." (no champion's kill); "That's an ace!" and "Your
  team is down."; turrets and inhibitors by whose; an inhibitor rising soon (fifteen seconds before)
  and standing again, each side's; a surrender, each side's (one that lost with its nexus standing
  gave up); the result. One moment's lines (a kill can call a multikill, a run broken, an ace and a
  new run) are said together, in order, under the weight of the biggest; a bigger moment waiting
  goes first; news more than 3 s old goes unsaid (5 s for big news), the end never.
- `el_client_core::fx::vo` (the champions): the script, a `Script` a champion with a casting note
  and its lines at eleven moments (pick, first move, three moves, two long moves, two attacks,
  encounter, ultimate ranked up, the ultimate's callout, two kills, death, respawn: 320 takes in
  all), each champion in its own voice (Vessa's dry watch, Orrin's hymns, Torek's reports...).
  `rule` is each moment's throttle and aim, Riot's numbers where they're known; `Chatter` keeps
  each champion's cooldowns, counts, takes (never one twice running) and whether it's talking.
- The director gives each moment to the chatter: the player's orders (`Client::order_log`), the
  casts, the deaths and kills, the respawn, the first sight of each enemy, the ultimate's rank.
  The rewards come from the own champion's state (last hits, kills, assists), past a high-water
  mark so a prediction put right never rings twice.
- In the browser: a sixth bus, the champions' voices, with its own slider (the menu's Sound page:
  Effects, Champions, Announcer, Interface, Ambience, Music). A champion's lines load once a match
  or champion select deals it; the pick line plays as champion select deals, rerolls or swaps.
  `hud_banner.rs` shows the announcer's line. `window.__el` has `vo_lines`, `vo_missing`,
  `vo_dropped`, `vo_last`, `vo_talking`, `banner` and `banner_shown`.
- The sounds: `gold.coin` (three takes), `takedown.x1` to `x5` and `takedown.assist`, synthesised
  by `art/tools/sounds.py` (the interface's bus: unplaced, the player's alone).

### The placeholders, and replacing them

`art/tools/voices.py` speaks every line with Piper, a neural text-to-speech (the announcer
`en_GB-cori-high`, public domain; each champion a speaker of `en_GB-vctk-medium`, CC BY 4.0),
cast as Riot casts a roster, cut to what text-to-speech can do: twenty speakers whose measured
pitches don't sit on top of each other (male 81 to 149 Hz, female 158 to 248), each pitched and
paced to its casting note, then processed as Riot layers a processed voice: the family's traits
quiet under a voice that stays clear (frost and starlight an octave-up shimmer; night and the
deep a muffled octave down; tide a slow chorus; wind a breath of air; bronze a metal comb; the
siege engine ring-modulated and crushed; a quiet knife whispered). `--listen` runs a speech
recogniser over every file written and scores the words heard against the words meant: the
nearest thing to a listener this pass has.

To replace a line, record its `says` from `web/assets/audio/manifest.json` (the casting note is
`cast`, the moment `when`, who hears it `heard_by`), and drop `<stem>.ogg` (Opus) and
`<stem>.mp3` into `web/assets/audio/` with a line in `art/LICENSES.md`. `voices.py` keeps the hash
of every placeholder it wrote (`art/voices.lock.json`) and never writes over a file that isn't one
of its own, so a recording is never lost to a rerun; a placeholder whose words or casting changed
is made again.

**What the recogniser heard.** The first full render, against the first script: lines heard
as other words where a voice or its processing muddied them (Orrin's hall took him from 62% of
his words heard to 48%, Seren's from 84% to 64%), and lines a person would mishear too: one-word
barks out of context ("Burn.", "Ring!", "Tolled."), and Gorum's ultimate, "Unbroken!", heard as
"I'm broken." From it: the long rooms shortened and the layers made quieter, the weakest six
voices auditioned against others over their whole scripts (five recast), and the hard lines
rewritten in two to four words, still in each champion's voice ("Nothing breaks me!"). The
render shipped: 364 lines, 91% alike on average (the letters heard against the letters meant),
212 word for word, 301 at 80% or more; 17 the recogniser didn't follow, listed by the tool to be
listened to (Piper's voices vary a little each render, so which seventeen moves). 1.7 MiB in
Opus, 2.35 in MP3 (speech at 24 kHz), all twenty champions and the announcer; a match loads its
ten and the announcer.

### Not yet

- Nobody has listened: the recogniser has (`voices.py --listen`), and a person should.
- Efforts: Riot's grunts and shouts for each spell, kept distinct "by their energy, length and
  their beginning consonant". Text-to-speech can't make them; they're an actor's.
- Emotes (taunt, joke, laugh, dance): the game has no keys for them yet.
- Interactions: lines that answer another champion (Riot's context lines), a first encounter
  with a particular champion, kill lines for a particular victim. Item lines Riot has retired.
- Captions for what's heard and not seen (13.19's global sounds beside the kill callouts): the
  banner shows the announcer, not the champions' callouts.
- A real announcer and twenty actors. The manifest is their script, the casting notes their brief.
- The music.

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
  source, `Heal` and `Area` with theirs, and two flags, empowered and hasted (`FLAG_BITS` 12; 13 since the ally frames' `ULT_READY`). A
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

**Steps 2, 3 and 5's first pass: the runtime, the sound, a look for everything.**
- `el_client_core::fx`, tested natively:
  - `sound`: the table of every sound (128 ours, 226 takes; 25 lines for the announcer and the
    music, optional). `web/assets/audio/manifest.json` is written from it, and a test fails when
    it's stale or a take of ours is missing.
  - `look`: a look for every source, derived from the ability as the sim's effect is (how it
    reaches gives its parts; its ops, Riot's four things, its ratings for the caster, allies and
    enemies; its champion's family, its palette and sounds). Tests: every ability of every
    champion has one, a blast that lands late warns by ear, an ultimate has its layer, and what
    stuns is rated over what pokes.
  - `director`: the cues of a frame. Every `WireEvent` has its own line, with no catch-all. The
    own champion's casts, blinks, missiles and swings come from the prediction, captured on a
    tick's first step only (`Client::own_feed`), and the server's word of them isn't played
    again. Anything about another unit waits for the drawn clock (at most half a second). Flags
    that rise become statuses, a turret turning on the viewer is heard, and the own champion's
    interface (a point spent, an item, the cards, an augment, a reroll) is heard from its state.
  - `mix`: the ladder (the viewer's own; enemies' warnings and ultimates; enemies' casts and hits;
    allies'; the world), a cap per sound and 32 voices in all, the lower rungs ducking 5 dB under a
    big moment and 3 under a crowd, takes that never repeat back to back, a pitch within 40 cents,
    and where a sound sits from where it is on the screen.
  - `voice`: the announcer's lines, aimed from the viewer's side (first blood, a streak of two to
    five, an ace, structures by whose they were, an inhibitor back, the minions, the result), one
    at a time, the weightiest first, news more than 3 s old dropped. And the music's states.
- The effects (`el-client/src/fx.rs`, `fx/`):
  - Bursts are closed forms: a pool of meshes whose vertices hold each particle's whole life
    (`shaders/fx_burst.wgsl`: glows, streaks, shards, puffs, rings, columns, flashes). Each family
    has its recipe for a hit (ember sparks fall, frost shatters, tide splashes, stone throws
    rubble and dust...), and every part has its own (cast motes, launch, impact, blast with its
    ring, windup motes drawn in, dash puffs along the path, buff spiral, deaths by kind, levels,
    relics). Size and brightness follow the cue's rating for the viewer.
  - What lasts is a pool of quads (`shaders/fx_live.wgsl`). Missiles get a head and a trail in
    place of the spheres: as wide as the lane, as long as the missile is fast. Zones get a
    telegraph that fills toward the moment it lands, or a field that churns and pulses, drawn
    from the wire's radius. An enemy's rim is hot and full, an ally's cool and fainter. Statuses
    are drawn from the flags: stars, a root's ring and binding, the silence and reveal marks, a
    shield's bubble, glows at the feet for a slow, a haste, strength and Combo Breaker.
  - The camera shakes a little when something heavy lands on the viewer's champion.
  - `?quality=low` keeps half the particles.
- A hit flashes a minion as it does a champion: each minion material's flash steps are materials
  of their own, shared as the rest are (`minions.rs`); in a browser match, 149 minions flashed.
- A match watched or replayed (`?watch=`, `?replay=`) is shown and heard as it was played.
  `el_client_core::spectator` reads each tick the page steps from the record as replication
  reads it for a team, with every unit seen, into a feed, missiles and zones shaped as a
  client's; the director reads either through its `Scene`, and the missiles, telegraphs,
  fields, statuses and the champions' casts are drawn from it.
  - The kill feed reads it too, naming the champions (a record names no players).
  - The watcher has no champion: Dawn is the near side, as the match is drawn, and nothing is
    the watcher's own. The announcer calls the fights from Dawn's side and says no victory. A
    team's pings and its vote stay the team's.
  - A tick stepped more than a second behind the one drawn (a live match caught up on) keeps its
    missiles and zones and lets its events go. Over 2x a match is drawn and not heard: at 16x
    its sounds filled all 32 voices.
  - In the browser (`scripts/e2e.sh watch`): a live match watched a minute behind is shown and
    heard within seconds of the fighting (one sample: 42 cues, 7 missiles in flight, a
    telegraph, 9 casts posed, 30 sounds); its replay, run at 16x, had 163 cues and 12 casts by
    1:49, every hash checked, none of its sounds failing.
- The sound (`el-client/src/audio.rs`) is Web Audio through `web-sys`, with no decoder in the
  wasm.
  - Every effect is its own source, then a low pass, its gain, and an equal-power panner, into
    the front or back of the effects bus. The five buses go to the master, then a limiter.
  - Where a sound sits comes from projecting where it happens through the camera: panned across
    the screen (never fully into one ear), quieter toward the rim, fading and dulling over half a
    screen past it, then nothing.
  - A wind bed, and the nearest brazier's crackle, louder as the camera comes near and panned
    where it stands.
  - Opus where the browser plays it, else MP3, from `assets/audio/files.json` (what's there:
    `build-web.sh`). The shared sounds load with the page, a family's once a match deals one of
    its champions. The first click or key resumes the audio, as browsers ask.
  - The menu's Sound page (Esc, or the button at the top left: `el-client/src/menu.rs`; a panel
    on the page, `web/audio.js`, until the menu) with the master and each bus, kept in
    `localStorage` (a test sets them through the inbox: `{t:"volume",bus,v}`, `{t:"mute",on}`).
- The sounds (`art/tools/sounds.py`, `art/LICENSES.md`):
  - The physical bodies are recordings from five of Kenney's CC0 packs, pinned by hash, their
    licence checked. They're kept in the repository (`art/vendor/kenney/`) since kenney.nl moved
    its downloads behind signed links; rebuilt from them, the sounds come out the same bytes. The elements are synthesised: fire's roar and crackle, water, wind bands,
    bell and glass partials, starlight, plucked strings, the warnings (rising sweeps, a shell's
    falling whistle).
  - Everything shares one small room, then is mastered by bus. A take's randomness is seeded by
    its name.
  - 1.6 MiB in Opus, 1.8 in MP3. Every file is checked for its codec, its peak (at most -1 dBFS
    after encoding), its onset (within 5 ms of its start) and its level.
- The announcer and the music, ready for their files: drop `voice.<line>.ogg` and `.mp3` (or
  `music.<state>`) into `web/assets/audio/`, give them a line in `art/LICENSES.md`, and rebuild.
  The names and what each line says are in the manifest. A line without its file goes unsaid;
  the music dips under a line.
- In the browser (`scripts/e2e.sh match`, headless Chromium):
  - The audio runs, and 178 sounds load with none failed.
  - Taking an augment is heard, and Blink is drawn and heard from the prediction.
  - Three minutes of a match: 2,632 cues, 66 sounds heard (the rest were off the screen, so never
    started), at most 12 at once, none dropped, none missing.
  - The walk is still predicted exactly (0.00).

**Step 4, practice, and the `fx` suite.**
- `el_sim::practice`: a match made practice after its reset (`Match::set_practice`), its record
  saying so in a frame after the header (`record::Frame::Practice`), so it replays as it was
  played. Seat 0 is the player's at the top level with every rank, nothing on cooldown and its
  mana full (kept so at the end of every tick, the server's and the player's own prediction
  alike), with 50,000 gold. The other nine are dummies at the Causeway's middle, three of Dawn's
  beside the player and five of Dusk's across from it (`PLACES`): they stand, walk a loop, or
  (seat 7) are shielded every four seconds; one pulled or knocked off its place walks back to it;
  they never fight back, and no champion dies (brought to nothing, it's made whole). The gates are open from the start and no minions march. Nothing in it
  runs in a match that isn't practice: the golden hashes held, and a record's practice frame taken
  out makes the same inputs go astray (`el-sim/tests/practice.rs`).
- `el-server --practice` holds them: the lobby's `ToServer::Practice { champ }` starts one at once
  (no champion select) with that champion in seat 0, and asked again starts a new one; one ends as
  soon as its player leaves. The page asks on `?practice=1` (`&champ=`), and again on
  `{t:"practice_champ", champ}`; a page that dials again asks for the champion it last asked for.
  The client makes its copies of the match practice too (`ToClient::Match`'s `practice`), so its
  walk is still predicted exactly (`el-match/tests/record.rs`).
- On the way, two faults the tests found: the dummies' walking orders weren't on the wire's grid,
  as every order is, so a practice record didn't replay; and the gates held the player behind its
  base for the first fifteen seconds.
- `scripts/e2e.sh fx`: every ability of every champion cast in practice at what it's for (the
  dummy in front, an ally, a point, itself), each counted as the player's own cast drawn
  (`fx_own_casts`) and heard (`audio_own_casts`), two pictures of each, and a contact sheet a
  champion and one of all of them in `e2e/artifacts/` (`EL_FX_FROM` and `EL_FX_CHAMPIONS` take a
  part of the pool). In SwiftShader, about four minutes a champion: all eighty casts drawn and
  heard, in two runs of ten (39 and 33 minutes), no sound failed, at most 10 at once, none
  dropped.
  There, a new practice match's ten figures stall the page past the server's 20 s of quiet,
  nearly every switch: the server lets the session go, and the page dials again and asks for the
  champion it last asked for (so a champion's first cast, lost with the match it was cast in, is
  cast again in the new one). Now and then a switch seemed never to reach the page (the server
  seated it; the page went on with the last match), so the suite asks again after 45 s, casts
  each ability up to three times, and the page logs each match it's seated in. The seated message
  wasn't lost: the page took it, then fed the new match's client the last match's snapshots that
  had queued behind the slow frame. Those set its clock to the old match's minute (even the ones
  that didn't decode), or its champion back to the old one, so every command it sent was for a tick
  far in the server's future, and with datagrams still coming it never dialled again. It showed
  only when a switch didn't also stall past the 20 s, since the redial after a stall started
  clean. Each seating's snapshots now come on a stream the lobby names, and the client takes no
  other (`NETCODE.md`, "Many matches a process").
- From the sheets: a shield's bubble had a middle full enough to hide the unit inside it (its
  middle is all but clear now, its rim tighter), and a dummy hooked to the player stayed there,
  so what was cast at its place after hit nothing (dummies walk back now). Little else can be
  judged from them: at a frame every two seconds most bursts fall between pictures.

**The particles, looked at.** `{t:"time",speed}` in the inbox runs the picture's clock slower or
stops it (`speed: 0`) while the match's own clock goes on, so a burst can be caught mid-life and
held there for a picture however long SwiftShader takes over it: the first pictures of bursts this
pass had. From them:
- `shaders/fx_burst.wgsl`: every particle has a seed of its own (where and when it was born). Light
  is born white-hot and cools to its colour; a glow is a hot core in a soft halo; a spark tapers
  from a bright head; a shard is lit on two facets and glints as it turns; a puff billows with a
  torn edge and turns as it grows; a shockwave has a sharp leading edge and a soft wake; a column
  has streaks rising up it; a ground flash is hot in the middle and boils outward, no flat disc.
  New shapes: a star of rays where a hit lands and at a missile's muzzle, embers that flicker and
  wander as they rise after fire, and a scorch of soot left under fire and stone blasts,
  ultimates and fallen structures.
- Smoke and dust are lit as the stone round them is: drawn as their own colour, a pale puff was a
  white cloud in the dark (a white ring round every fire blast). The first scorch was a solid
  black disc as wide as the blast, a pit in the floor; it's soot in patches now, half that size.
- `fx/light.rs`: six point lights, flared where a blast lands, an ultimate is cast, a heavy hit
  strikes or a structure falls, and fading in a fraction of a second, so the stone and the units
  round it take the effect's colour (the high tier).
- A burst holds 48 particles, and its hit's core and star go first, so a full one never drops them.
- Not caught by it: missiles and small hits. They fly on the match's clock, not the picture's,
  and are over between two of SwiftShader's frames (the catch, below, is for them).

**Missiles and hits, caught.** `{t:"catch",what,at}` in the inbox holds the picture on the next
missile spawned, drawn at `at` of its flight (the client's table keeps the last spawn, so one whose
whole flight fell between two frames is still drawn), or on the next cue of the parts named
(`impact,blast,buff,dash`), its bursts and light born `at` seconds ago; the picture's clock stops
there, nothing new is born in it, and `{t:"time"}` lets it go (`fx.rs`, `Catch`; `fx_caught_what`
names what it took). The match's own clock goes on, so units still walk. `scripts/e2e.sh fx` casts
each ability twice more for it, a picture of its missile halfway and one of what it does as it
lands, a tenth of a second old. What the first pictures showed:
- A skillshot's hit was invisible. Its core and star were made at the unit's chest, in the middle
  of its body, and the model's depth hid them; only what flew out of it showed, and the frost
  shards a tenth of a second out were a few pixels. A hit, and a launch's muzzle, are drawn on
  the body's near side now, out toward the camera by the unit's drawn radius: the star shows on
  the dummy at 0.02 and 0.06 s and is gone by 0.12, as it's made to be.
- A particle fades in over its first 6% of life and is nothing at age 0, so a burst born while
  the clock is stopped never shows; a catch's age is what puts it in the picture.
- A missile's head is a hot core (1.6 times its colour) in a faint halo of its edge, so a pale
  family's (Vessa's frost) reads as white in flight; an ember's (Orrin's) reads as fire. One for
  the hand-tuning, as is a blast's ground flash on the low tier, still more a soft disc than a boil.
- A catch takes only what an ability does: Vessa's E dashes her to the dummy, and the first run's
  "missile in flight" after it was her auto-attack.
Measured over the first three champions: sixteen pictures held over their twelve abilities, the
four missiles among them and what each of the twelve does as it lands (an impact, a blast, a buff
or a dash), about six minutes a champion where it was about five, the walk still exact.

Then all twenty (the suite in four runs, 3 to 8, 9 to 14 and 15 to 19 after the first three, each
passed: about five minutes a champion, no cast missed, no sound failed, at most 11 at once, the
walk exact): 19 missiles held in flight, and 78 of the 80 abilities held as they land. The two
that weren't: Sefa's Q, a missile of 0.17 s that ends short of the dummy and lands on nothing,
and Brun's E, whose missile does nothing a catch takes (no impact, blast, buff or dash). From the
pictures, for the hand-tuning:
- In flight, the families read apart (fire orange, frost blue, dusk magenta, steel a white
  streak); Vessa's and Marret's W are pale blobs.
- Blasts and ultimates read at once; a single target's hit is the smallest thing in the picture
  (a star the size of the unit's head), and a buff on the caster at a tenth of a second is
  barely there (Ulla's three).
- Liora's R lays a dusk cloud under its target, dark as a shadow: it reads, but as a hole more
  than a strike.

**Step 7, the stage and the flipbooks.** The loop the hand-tuning was missing, taken from Gates
as it is (`AnthonE/Gates`, `crates/fx/src/stage.rs` and `ci/fxstrip.mjs`), and oneshot's baked
smoke (`AnthonE/oneshot`, `tools/flipbooks/bake.py`), cut for the Causeway.
- `?fxstage=orrin-q-impact` (`scripts/fx.sh list` names 296: every part of every ability, each
  champion's auto, Blink, Tag, the turret's shot, the statuses, the deaths, a respawn, a level,
  the relic) fires the one effect twice at once over a dark card and a light card laid on the
  Causeway's middle, on a clock stepped exactly 1/60 s a frame and held where `window.__fxStep`
  asks; `scripts/fx.sh <name>` shoots it with Gates' `fxstrip.mjs` into a strip with its numbers
  (`e2e/artifacts/fx/<name>/`). The stage needs no server (it makes the cue the director would and
  hands it to the same bursts), hides everything of the Causeway each frame (the chasm's spires,
  9 m tall, stood over the cards' far edge before), looks at the effect's own height (a hit is
  drawn on a body's near side, a metre and a half up and toward the camera: framed from the floor
  it sat in the frame's corner), and its cards read 26 and 183 of 255 through the tonemapping (the
  stage's 25 and 185). A strip of twelve frames over a second and a half takes about a minute in
  headless Chromium, the frames exact at any frame rate; the suite's five minutes a champion were
  for the match's own clock. `--knobs 'impact.star=1.5;size=0.8'` retunes a recipe's numbers
  without the four-minute wasm rebuild (`scripts/fx.sh knobs` lists the twenty-two).
- `art/tools/flipbooks.py` bakes one 1024² sheet pair (8 × 8 cells of 128: a smoke puff's sixteen
  frames twice, a fireball's eight twice, a flash of petals' four, four times) in fifteen seconds
  from a 256² fluid simulation, lit six ways; 460 KB as PNG, where the models are 3.3 MiB a match.
  The burst shader plays them for a puff, a blast's fireball and a hit's flash: the frame for its
  age and the next crossfaded, the smoke lit from where the moon stands in the sprite's own frame
  at about the level the stone reads (the lights' own numbers, lux through the camera's exposure,
  are far brighter: the ground's shader grades its own), eroded from its thin parts past half its
  life, the heat added through the family's ramp (its edge colour as it warms, its core as it
  burns, white only at the very top: a ramp that went white early made a cloud of every fireball).
- From the strips, against the suite's findings: a single target's hit (Orrin's Q) is a flash of
  petals now, as wide as the old star and gone as fast (peak 244 of 255 at 0.05 s, 1.8% of the
  dark card); its smoke a lit, lumpy puff with a bright side and a dark one where it was a flat
  grey blob. A buff (Ulla's E) blooms at the chest as it lands, so it shows at a sixtieth of a
  second and peaks at 0.083 s where it peaked at 0.25 s, its ring and motes rising after. Liora's
  R leaves its light on the ground, fading over two seconds, where a scorch of soot
  read as a hole (a scorch is fire's and stone's now, ultimate or not). A blast's fireball burns to
  soot where fire strikes and is the heat alone, an energy burst, for frost, dusk, tide, wind,
  growth, bronze and steel; stone throws dust and rubble and no fire. A shockwave's wake is a band
  behind its edge, not a fill: at its birth it filled the whole blast with a pale cloud of the
  family's edge colour, which read as smoke and wasn't. The ground flash is dimmer, so its boil
  shows (Brakka's E: a cracked plate of light, peak 216 where it was 232 flat), and stone's dust
  rolls out pale under the moon round it.

- A second pass, thirty-eight strips across the families and the world's effects (one of each
  part a family, the statuses, the deaths, a respawn, the relic, the turret), and the missiles
  once they could be staged. What read as it should: every family's hit (a flash of petals in
  its colour and what flies off it: steel's white streaks, frost's shards, tide's foam, bronze's
  binding ring), the energy bursts (frost white-blue, dusk violet, steel white), the ultimates'
  rings and columns, a root's ring and vines, a shield's and a heal's rings, a turret's hit, a
  champion's and a nexus's fall. What didn't, and was changed: a cast's ground flash was wide
  enough to read as pink smoke (smaller, quicker, a hot middle); the dust off a stone hit and
  the foam off a tide hit outgrew and outlasted the hit (Brakka's Q peaked at 0.6 s on its dust
  at 13.8 of light, the strike itself at 3; now 3.0 at 0.47 s and 4.4% of the card where it was
  17.6%); a knock-up's six wide puffs outlasted the lift; and a missile's head was a wide soft
  core in a halo as wide as its quad, with a trail as wide as its lane (Vessa's W, the suite's
  pale blob: a hard point half its lane wide in a tight halo now, the trail a narrow streak
  bright only at the head; 5.5% of the dark card where it was 8.8%).

- A third pass, on what Gates' engine and oneshot's effects learned after the stage came over
  (`AnthonE/Gates` `crates/fx`, `AnthonE/oneshot` `crates/client/src/look/fx`), seven strips before
  and after (Vessa's Q and W, Orrin's Q, Brakka's Q, Brun's Q, Liora's R, a turret's fall):
  - Light covered what was behind it. Bevy blends an added material premultiplied
    (`src + dst × (1 − a)`: `AlphaMode::Add` and `Premultiplied` are one pipeline), and only its
    standard material writes no alpha for it; the bursts' light, the live pool's missiles,
    telegraphs, fields and statuses, and the braziers' flame all wrote theirs. So a spark's core
    blotted out what it crossed, light over light didn't add up, a telegraph's fill darkened the
    floor it lay on, and a missile's head was a white ring round a grey middle (its core covered
    the trail under it, and the tonemapped core alone was dimmer than core and trail together).
    They write none now, as oneshot's shock shell, tracers and plasma do. On the stage: Vessa's
    W's head a solid point; a hit no longer greys the light card round it (Vessa's Q took 0.06
    of light from it, none now); the dark card's peaks a little higher at the same moments
    (Vessa's Q 1.58 to 1.85, Orrin's Q 1.33 to 1.60, Brun's Q 12.99 to 13.54, Liora's R 23.53 to
    24.23), every strip's peak, half and last-seen times as they were. Where a hit's parts
    overlap they add now: its core clips for its first two frames (8 to 18% of what changed).
  - The shield's rim took `pow` of `1 − |n·v|`, a hair under zero where the sphere faces the eye
    and the dot rounds past 1: a NaN, which blends to black (oneshot's shock shell had it).
    Clamped.
  - A spark is Gates' true line: its head where it is, its tail where it was 25 ms before,
    foreshortened toward the eye. It was centred on itself and a few times as long as wide at
    any speed, a dot more than a streak at 27 m.
  - A spark's line is never drawn under 1.6 px across, nor a glow, a shard or an ember under
    2.5, each dimmed by what it was widened (oneshot's tracers): without multisampling (the low
    tier) they broke into dots.
  - What's thrown down (a spark, a blast's motes, rubble) strikes the floor once and lies on it
    (Gates' bounce, solved in the shader: the moment it struck found by halving), where it fell
    through the floor and out of the picture.
  - A sheet's sprite (a puff, a fireball, a flash of petals) fades in its last 0.4 m over the
    floor, oneshot's soft edge against the depth behind: the Causeway's floor is flat, so its
    height is that depth, and a depth prepass would take textures the ground's pipeline needs.
  - A burst's slot is written in place, its unused quads where it starts: Bevy sorts what's laid
    over by its mesh's bounds, and quads left at the scene's zero stretched every burst's to the
    middle of the map, so smoke lay over smoke in no order. Light sorts after smoke at the same
    place, as Gates' two pools draw. A slot taken from a burst still burning is counted
    (`fx_bursts_stolen`), as Gates' pools count what they steal.

Not yet:
- Nobody has listened: the sounds were judged by their spectrograms and measures, not by ear. The
  look on a real GPU hasn't been seen either (SwiftShader draws a frame every two seconds, so a
  burst is seldom in a match's picture; the stage's are exact).
- The per-ability hand-tuning step 5 ends with: the stage and the strips are its loop now, and
  about fifty effects have been through it (one of each kind a family, and what they showed was
  fixed in the recipes every champion of the family shares); the rest haven't been looked at one
  by one.
- A champion's model under a hit on the stage, to judge it against a body.
- Step 6's frame numbers.
- From Gates and oneshot, not taken yet: a pool in one mesh (Gates' world is two draws; ours is a
  draw a live burst on WebGL2), oneshot's shock shell round a blast, its lit debris, and its
  tracers' screen-space lines, held in pixels, for a missile's trail.

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
- Riot Games, [/dev: The Voice of the Visionary](https://www.leagueoflegends.com/en-us/news/dev/dev-the-voice-of-the-visionary/) (2023).
- Riot Games, [The Frequencies of Folklore](https://www.leagueoflegends.com/en-us/news/dev/the-frequencies-of-folklore/) (2020).
- Riot Games, patch notes [10.2](https://www.leagueoflegends.com/en-us/news/game-updates/patch-10-2-notes/)
  (fog), [13.19](https://www.leagueoflegends.com/en-us/news/game-updates/patch-13-19-notes/)
  (executions in fog, global sounds) and [25.11](https://www.leagueoflegends.com/en-us/news/game-updates/patch-25-11-notes/)
  (kill and takedown sounds).
- The League of Legends Wiki (a fan wiki, datamined): [Announcer](https://wiki.leagueoflegends.com/en-us/Announcer),
  [Announcer/Thresh](https://wiki.leagueoflegends.com/en-us/Announcer/Thresh), [Quote](https://wiki.leagueoflegends.com/en-us/Quote),
  [Vi/Audio](https://wiki.leagueoflegends.com/en-us/Vi/Audio), [Mel/Audio](https://wiki.leagueoflegends.com/en-us/Mel/Audio).
- Player One, [Sandy Zhou: designing iconic combat sound](https://www.player.one/ip-half-second-how-sandy-zhou-designs-iconic-combat-sound-league-legends-162346)
  (a contributor's piece, not Riot's own).

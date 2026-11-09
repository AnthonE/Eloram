# The champions, drawn

The plan for the twenty champions' figures: a model each, rigged and animated, with what it holds,
moving as the game says it moves. Until this pass a champion was a figure of Bevy's primitives,
one a role, in its champion's colour (`el-client/src/models.rs`, still the stand-in while a model
loads). Nothing of Riot's is traced, sampled or named (`CLAUDE.md`).

## What Riot says makes a champion read

- **The silhouette first.** "Silhouettes are the main driver of champion recognition", basically
  a champion's shadow, and a good one has a "defining primary characteristic that's unique to that
  champion", one no skin may take away: Senna's gun. The silhouette shows the power source (a bow
  says ranged, a staff says spells), size hints at the role (bulk for tanks and fighters), and
  which way a champion faces reads from the outline alone. Animation carries the rest: Lee Sin,
  whose shape is generic, is "carried by his animations" (Clarity in League, Ask Riot).
- **The hitbox is where the champion is.** Cosmetic animation never has a champion "abandoning"
  its hitbox: Pool Party Renekton's recall that rose out of it was called out (Clarity in League).
- **Animation is the game's tell, timed to it.** League's champions are hand-keyed, not captured,
  for "iconic character silhouettes for gameplay readability" and "smooth and instant animations
  that don't affect League's gameplay"; a cast of one second is thirty frames, and "it was
  absolutely vital the animation did not exceed this time". Poses too small, hidden by effects,
  were redone (Champion Animation, 2014).
- **An attack is a windup, then the rest.** A basic attack's time is its windup and its cooldown;
  the windup is a champion's share of the attack's time, and it scales with attack speed; an order
  during the windup cancels the attack (the League wiki, from Riot's numbers). The animation keeps
  that rhythm: its strike lands as the windup ends.
- **What a champion is made of.** A skinned mesh, each vertex weighted to one to four of fewer
  than a hundred bones; Garen is 6,336 triangles; champions are drawn first among the skinned
  meshes and cast shadows (A trip down the LoL graphics pipeline). Their animations are stored as
  quantised quaternions, 48 bits a rotation (Compressing skeletal animation data). Sion has 38.

## What we build

- **A model a champion**, each with its primary feature in its hands or on its back: Vessa's long
  crossbow, Marret's blunderbuss, Orrin's censer staff, Seren's star wand, Torek's mortar on his
  shoulder, Brakka's chain hook, Hollis's mace and bell, Talmar's trident, Vire's greatsword,
  Pell's sword, shield and banner, Brun's forge hammer, Sefa's crescent blade, Kesh's two knives,
  Sable's shade lantern, Liora's lantern and halo, Oda's staff and pack, Ulla's living staff,
  Mira's lamplighter's pole; Nimra's ice crown and Gorum's ox-skull helm are their own.
- **Twenty clips**, a kit each (`el_client_core::pose`): idle, run, a flinch (looped while
  stunned), death, thrown up, a roll or a charge for its dash, victory; an attack by what it holds
  (a slash, a thrust, an overhead smash, a punch, a shot from the hip, a two-handed bolt, a flick
  of the hand); its four casts from how each ability reaches (aimed, gathered and released, raised
  high, slammed down, a great stroke for a bladed ultimate), as `fx::look` derives an ability's look.
  Eighteen more came later from a CC0 library ("The second clips"): a second attack each, stances,
  flinches, a throw.
- **Timed to the game.** An attack's clip plays so its strike lands as the server's windup ends:
  the own champion's windup is known to the tick (the prediction has it), anyone else's is its
  champion's at its level, then the last one seen (attack speed bought shows in it). A cast's
  release lands as its ability's windup does. A run's stride keeps pace with the floor. A
  champion that moves cancels into its run, as an order cancels a windup.
- **The body stays in its hitbox.** Every clip's hips rise and fall but never travel across the
  floor: the unit's place is the server's.
- **What happened shows.** A champion seen to fall lies where it fell, playing its death, until it
  lives again; a hit flashes it white; the winners cheer.

## The assets

Meshy, on this environment's proxy, with its rigging and animation as well as its text-to-3D:

1. **The champions** (`art/meshy.json`, `champions`): text-to-3D (`meshy-6-lite`) in an A-pose,
   remeshed to 8,000 triangles, textured from a second prompt, then rigged (a 24-bone humanoid
   skeleton, its own run and walk with it). A model made in an A-pose drops what it holds and most
   of what's on its back, so those became props.
2. **The props** (`art/meshy.json`, `props`): an untextured mesh each (a preview, the cheapest
   task), coloured by bands along its length in the pipeline (`art/champions.json`): wood, steel,
   brass, the glowing bands lit.
3. **The clips** (`art/meshy.json`, `animations`): nineteen of Meshy's library, made once on one
   champion's rig and retargeted onto every other: each bone's turn from its rest pose, in the
   world, put on the other body's rest pose. Checked against Meshy's own retarget of the same clip
   onto another rig (Brakka's slash): the same pose frame for frame.

The budget was the account's 650 credits: 400 for the twenty champions (5 + 10 + 5 each), 57 for
the nineteen clips, 125 for twenty-five props (a crescent and antlers didn't come out and aren't
used), 12 for the clips tried while finding the way; 56 were left, and the minions took 55 of
them ("The minions", below).

## The pipeline

`art/tools/meshy.py` makes the tasks and fetches their outputs (each task's id kept the moment it's
made, so a run that stops is taken up and nothing is paid for twice); `art/tools/champions.mjs`
(glTF Transform, `npm ci --prefix art/tools`) makes `web/assets/models/champions/<name>.glb`:

- the champion's own run and the clips its kit plays (`art/clips.json`, written from
  `el_client_core::pose` and checked by a test), each cut to its window (`art/champions.json`,
  `clips`) and retargeted;
- the props hung on their bones, pointing where `art/champions.json` says at rest;
- simplified to about Riot's six thousand triangles, the skin's weights as bytes, its texture
  coordinates as 16-bit integers, rotations as 16-bit integers (glTF's own normalised types, which
  Bevy widens as it loads), the texture cut to 512 px JPEG;
- stood one tall, feet at zero, facing +Z: the client sizes and turns it.

## The runtime

- `el_client_core::pose`, tested natively: the clip table and each champion's kit, and a `Poser`
  a champion that takes what the client knows of it each frame (alive, its speed, its flags, the
  ability it's casting, its windup if known, whether its team won) and the casts it began (the
  events) and says which clip, how far in, and the clip it's fading out of. The states outrank
  the actions: dead, thrown up, stunned, carried by a dash; then a cast, an attack, a run, idle.
- `el-client/src/champions.rs`: a champion's model is fetched once the match deals it (the team's
  own already in champion select), its scene spawned under the unit with the clips in an
  animation graph; each frame the pose is played on it, paused at the pose's time (the clock is
  the pose's), two clips blended while one fades. The primitive figure stands in until it's there.

## The motion

The first pass gave every champion a model, clips timed to the game, and nothing between "the
right clip at the right moment" and a champion that feels alive. Seen in a match: all twenty moved
alike (the same nineteen clips, their windows shared), each clip played at one even pace (a 1.3 s
captured slash squeezed into a 0.3 s windup winds up and follows through at the same speed), a
champion snapped to its facing at the sim's 30 Hz (a path round a minion turns it every tick), a
hit was a white flash and nothing in the body, and nothing came from what it held. Riot's word is
that the hand-keyed look is "smooth and instant", the silhouette carried by poses that read; this
is what we built for it, client-side, with the sim, the wire and the record untouched.

- **The timing's shape** (`el_client_core::pose`): a strike's wind-up is eased in, slow then
  fast into the hit (`Feel::snap`: the clip's time to its strike is `u` blended with `u³`), the
  strike holds on its contact for a few frames (`hold_melee`, `hold_ranged`, `hold_cast`), and the
  follow-through plays on at its own pace (`follow`). The strike still lands exactly as the
  server's windup ends; a test holds it for any knobs.
- **Two attacks, alternated**, as League's champions swing (`Kit::attacks`, by what each holds):
  a slash and a sweep, a cut and a slash, a thrust and the left knife's jab, a punch and a hook, a
  smash and a lunge, a two-handed shot and a one-handed one, a gesture and a spell.
- **A stance each** (`Kit::idle`): a blade's guard (Vire, Talmar, Sefa), a shield raised (Pell), a
  crouch (Kesh), a weapon held up to aim (Vessa, Marret), a light held up (Liora and Sable
  left-handed, Mira right), arms folded (Gorum); Meshy's idle for the rest.
- **The legs run while the arms cast** (`Pose::legs`): every clip is in a model's graph twice, the
  upper body and the hips and legs apart (Bevy's masks, `el-client/src/figure.rs`), so a cast
  walked through plays on the arms over the run, and its follow-through ends into the run without
  a hitch in the legs. Before, moving cut a cast's follow-through to the run outright.
- **A flinch** over the upper body when hit (`Pose::flinch`), a reel past `heavy` of its health,
  on top of the flash; not over a fall, a stun or a knock.
- **The motion** (`el_client_core::motion`): the figure turns to its facing in a few hundredths of
  a second, the short way, never past it (`turn`); leans into turns by how fast it turns and moves
  (`bank`), forward as it speeds up and a little as it runs (`lean_accel`, `lean_run`), back as it
  stops; is pushed away from whoever hit it by the hit's share of its health and springs back
  (`recoil`, `recoil_share`); and squashes and stretches about its feet, keeping its volume, as it
  lands, strikes, dashes or falls. The minions turn, lean and recoil the same way, and flinch.
- **The beats** (`pose::Beats`): a strike, an attack or a cast begun, a dash of its own, a landing,
  a fall, a hit; what the motion and the effects key off.
- **Its sockets** (`figure::Sockets`): its hands, its toes, and the tip of what it holds (an empty
  `<prop>.tip` node the pipeline puts at each prop's far end). From them: a swing's **arc**, a
  ribbon over the outer part of the blade over its last tenth of a second, in its family's colour
  (`el-client/src/trails.rs`); a **light gathering** at the hand or the staff's tip as a cast winds
  up; the **launch's flash** at the muzzle, and a missile drawn leaving it and on its own path a
  moment later (where it is and what it hits stay the server's); the **dust** the player's own
  champion's running foot kicks up as it comes down, and its **step**, heard (`step`, made by
  `sounds.py` from nothing recorded; everyone's steps took the burst pool's smoke slots from the
  effects in a watched match); a **landing's** ring and a **dash's** puff.
- **The look**: each champion's own colours a little richer than made (`shaders/rim.wgsl`,
  `look.x`: League's champions are the most saturated thing on screen), lit by its own colour
  warmly against the night's cold light (the subject warm, the world cool), its team's rim
  softened so it no longer drowns the texture (an ally read blue on the blue stone), what it holds
  in hand drawn a quarter bigger (the "defining primary characteristic", at some 60 px tall), and
  on the low tier, which has no shadows, a soft blob of shadow under it.

The numbers are a `motion::Feel`, `Feel::GAME` in a match; the anim stage's knobs retune them
without a rebuild.

## The second clips

Meshy charges a clip a rig, and its last credit was spent. Quaternius's Universal Animation
Library, volumes 1 and 2 (the Standard sets: CC0, some ninety clips between them on one humanoid
rig under two sets of bone names), has the rest: sword strokes with their recoveries, a jab, a
cross and a hook, a simple spell cast, a pistol shot, an overhand throw, flinches to the chest and
the head, and stances (a guard, a shield, a torch, a lantern, folded arms, a crouch, a weapon
raised). `python3 -I art/tools/ual.py` fetches both and checks them against their hashes and
their licences; `art/champions.json`'s `ual` names each clip we take, its parts in order (a
stroke then its recovery: one clip); `node art/tools/champions.mjs --ual` puts them on the models
already made (and the full build with the rest).

- **Retargeted across rigs** (`art/tools/retarget.mjs`): both rigs face +Z with their left on +X,
  but the library's rest is a T-pose and Meshy's an A-pose, so each bone's turn is taken from the
  library's rest after turning that rest onto the champion's, the shortest turn that lays each bone
  of one along the same bone of the other (a hand and a toe take their parent's). A bone of the
  champion then points where the library's does. The hips rise and fall by the library's share of
  hip height and never travel.
- **Their strikes measured**: where the weapon's tip (or the hand) moves fastest
  (`retarget.mjs`, `strike`), checked by hand where that's the wrong hand (the jab is the left)
  or the arm coming down (the spell's release is its shot's start).
- **Looked at without the browser**: `node art/tools/champions.mjs --sheet <glb> <png> [clip...]`
  draws each clip as a row of stick figures from the front and the side, the prop in gold and the
  strike boxed (a library's own clips too).
- The models keep exactly their kit's clips: a champion whose stance is a guard no longer carries
  Meshy's idle. The minions take the chest's flinch.

## The stage

How a champion moves can't be judged in a match in headless Chromium (SwiftShader draws a frame
every second or so; a swing falls between frames), nor from one still. The anim stage is the fx
stage's loop for it (`el-client/src/animstage.rs`, `el_client_core::anim`, `scripts/anim.sh`,
`.claude/skills/anim/SKILL.md`, `.claude/agents/anim-judge.md`): one champion doing one thing,
twice (in profile over a dark card, three-quarters toward the camera over a light one), on the fx
stage's clock (a step a sixtieth of a second, held where the shooter asks), shot by the same
`e2e/fxstrip.mjs` into a strip with a diff against the last run. Each act is a script of what a
match's client would tell the figure (`anim::Act`: stand, run, attack, each cast, a cast on the
run, a light and a heavy hit, turns, a landing, a fall, a win); 260 by name (`vire-attack`,
`seren-r`). `&knobs=` retunes the `Feel` without a rebuild. The figures are drawn as a match draws
them, with what they kick up and draw.

## The minions

The minions were the last primitives on the lane: a cube, a ball, a crate and a bigger cube. Riot's
word on them is short. When Summoner's Rift was remade in 2014 the new minions kept "the low
profile of their predecessors", and their "simple shapes mean they have a recognizable silhouette
and lumber to their walk" (Dev Blog: SR's gameplay updates, 2014). So they're smaller and plainer
than any champion, told apart at a glance by shape and by what they hold, and they walk.

- **Four models, two bodies** (`art/meshy.json`, `minions`; `art/champions.json`, `minions`): a
  squat helmed foot soldier with a sword and a kite shield (melee); a hooded acolyte with a staff
  (caster); the foot soldier again with a great hammer, drawn bigger (super); and a cannon on a
  four-wheeled cart (siege), which has no rig. The bodies are rigged by Meshy and carry the
  champions' shared clips, retargeted as the champions' are: idle, their own run, a flinch,
  thrown up, death and an attack (a slash, a flick of the staff, the hammer brought down).
- **About 2,000 triangles** a body before what it holds (2,500 to 3,000 with it), a 256 px
  texture, 115 to 300 KiB a model: there are dozens on the lane at once.
- **A team's colours on one model.** Every material is made once a team and shared by every minion
  of that kind and team. Meshy painted the team's colour blue (the caster's robe, the cart's
  pennant), and `art/tools/champions.mjs` writes `<name>.dusk.jpg` beside each model with the blues
  turned red; each team's materials lean a little toward its light; and a band of what each holds
  is the team's (the shield's face, the staff's stone, the hammer's head; the stone and the head
  glow), the props' `team` band, which the client colours. The melee soldier came back all steel,
  without the blue tabard asked for, so its shield carries its team.
- **Posed as the champions are** (`el_client_core::pose`, `Poser::minion`): an attack plays so its
  strike lands as the minion's windup ends (a share of its time between attacks,
  `el_sim::content::rules::MINION_WINDUP`: 11 ticks for a melee minion, 14 for a caster), a run
  keeps its stride to the floor, states outrank the attack, and a minion never casts or cheers.
  The cart rocks as it rolls and kicks back as its windup ends, when it fires, and its wheels
  turn as far as it rolled: `art/tools/champions.mjs` cuts them out of the cart (Meshy made it one
  mesh, the wheels welded on) into four nodes, `wheel0` to `wheel3`, each turning on its own X
  about its hub. A wheel is the triangles all within its radius (0.22 of the cart's height,
  measured off the mesh: every rim within a tenth of it) of a hub a radius over where it touches
  the floor; what reaches out of it, where the axle meets the bed, stays with the body
  (`node art/tools/champions.mjs --wheels <glb>` does it to a cart already made).
- **A hit flashes it** as a champion's does, fading in three steps: its materials are its kind's
  and team's, shared, so each step is a material of its own made with them, and a minion hit
  wears the step it's at.
- **Fallen** (`el-client/src/minions.rs`): a minion seen to die (its death told, or watched) plays
  its death where it was drawn, lies a moment and sinks into the floor; a new minion in the slot
  cuts it short. The primitive stands in until its model is in.
- The budget: 15 for three previews, looked at before anything else was paid for; 30 to texture
  them; 10 to rig the two bodies. One credit is left.

## How we know it's done

- **Coverage**: a test that every champion's model carries every clip its kit plays, at the
  length the clip table says, and that `art/clips.json` is the table's.
- **Timing**: tests that an attack's strike lands as the windup ends, the own champion's to the
  tick and anyone else's from its champion and then from what was seen; a cast's release with its
  ability; moving cuts a follow-through short; states outrank actions and fade in.
- **Pictures**: the `match` suite's screenshots with the champions in them.
- **The motion** (`pose`, `motion`, `anim`, natively): the strike still lands as the windup ends
  whatever the knobs; a strike eases in, holds and follows through; attacks alternate; a cast
  walked through keeps the legs running and ends into the run with the legs' cycle unbroken; a
  hit flinches the upper body and a heavy one reels it, never over a fall or a stun; the beats
  come once each; the turn takes the short way, fast, and never overshoots; the lean goes into a
  turn and forward as it speeds up; a hit pushes away from its source and comes back; squash keeps
  the volume; a long frame doesn't blow a spring up; every act on the stage does what it says.
- **The models hold exactly their kit's clips** (no more, none missing), each prop's tip beside
  it, and the library's clips at their lengths.
- **The stage's strips**: an act shot and read (`scripts/anim.sh`).
- **Nothing else moves**: the sim, the wire and the record untouched (no `CONTENT_VERSION` bump),
  the golden hashes, no allocation in a tick, `ci.sh` green.

## What landed

- Twenty models, 494 to 656 KiB each (6.6 MiB brotli for all twenty, about 3.3 for a match's ten),
  7,000 to 9,800 triangles with their props, nine to eleven clips each; twenty-three props from
  twenty-five made.
- In the browser (`scripts/e2e.sh match`, headless Chromium): the models in as the match starts
  (the team's five and a benched one already in champion select), the own champion standing and
  then running down the lane, the walk still predicted exactly, a fallen champion lying where it
  fell; 0.74 frames a second in SwiftShader, 0.77 before (its frame rate means nothing but that
  nothing got much slower).

- The minions (above): four models of 115 to 301 KiB with Dusk's four textures beside them (589
  KiB as served, brotli), 2,000 to 3,000 triangles each with what it holds, six clips each but the
  cart's none. In the browser (`scripts/e2e.sh match`) the four are in before the first wave, then
  twenty minions are drawn at once as models, walking and swinging; the walk still predicted
  exactly; 0.69 frames a second in SwiftShader, as before. Watched (`watch`, from a checkpoint
  after the first waves), they're drawn from the record as the champions are.

Changed on the way:
- **What's held became props.** A model made in an A-pose (which rigging needs) drops what it holds
  and most of what's on its back: the crossbow, the bell, the pack, the mortar, the banner were all
  gone. Meshy's untextured props, coloured in bands by the pipeline, cost a third of a textured one.
- **One rig's clips for all.** Meshy charges a clip a rig; twenty rigs would have taken most of the
  credits for the clips alone. Retargeted by each bone's turn from rest in the world, they match
  Meshy's own retarget.
- **Brun's hammer the right way round.** The forge hammer's mesh has its head at the end the
  pipeline takes for the foot, so Brun held it by the head with the handle out in front, from the
  start; the prop is flipped now (`art/champions.json`), and the super minion's hammer is the same.
- **Lit a little by their own colour.** Under the moon the textured champions read dark against
  the stone; each is lit by its own texture at a fifth of its colour, as Riot's skinned meshes have
  an emissive of their own.

- **A rim of its team's light** (`el-client/src/champions.rs`, `Rim`; `shaders/rim.wgsl`): each
  champion's material is the standard one with a fresnel term added to its emissive, in its
  team's light (Dawn's and Dusk's, `fx::look::world::TEAM`), brightest where its surface turns
  from the camera and kept a little underneath. Riot inks its champions' outlines so a silhouette
  reads; here the outline is lit, which needs no second pass and costs a few instructions a pixel
  of champion.
- The mode has no recall (`DESIGN.md`), so no champion needs one.

- **The motion** (above): the timing's shape, two attacks alternated, a stance each, the legs
  running under a cast, flinches, turning, leaning, recoil, squash and stretch, a swing's arc, a
  casting hand's light, shots from the muzzle, a running foot's dust and the player's own steps,
  a landing's ring and a dash's puff; the champions' own colours richer, warmly self-lit, a
  softer rim, what they hold a quarter bigger, a blob of shadow on the low tier. Eighteen clips
  from Quaternius's CC0 library on the models (two to six a champion; a flinch on each minion),
  595 KiB over the twenty models (30 KiB each; 10.95 MiB to 11.53 raw), the minions' as they were.
  Nothing on the wire, in the record or in the sim.
- **The anim stage** (above), with which it was tuned. What it showed:
  - Meshy's slash struck a tenth of a second early: at its strike the blade was still crossing
    the body, the swing after it. Measured where the tip moves fastest on every model that swings
    it, the slash is at 0.69 s (not 0.55) and the slam at 1.12 (not 1.05); the thrust, the smash,
    the punch and the stroke agree with the table to a hundredth.
  - The team's rim drowned the textures: Vire read blue-white over the dark card. At 0.55 (from
    0.9) and a little tighter, with the colours richer before the light (not after, where it
    saturated the rim too), his red cloth and grey steel read.
  - A swing's arc over two thirds of the blade, bright, was a white sheet, its fan reaching the
    next card on a fast follow-through; over the outer 40%, dimmer and quicker, it's a crescent.
  - A casting hand's light at 12 to 45 cm was six pixels: it's 30 cm to a metre now, brightening
    as it gathers.
  - An about-face is done in a few frames and never past; a weaving run banks into each turn.
- In the browser (`scripts/e2e.sh match`): the own champion standing in its stance (the suite
  waits for `window.__el.champion_stance`, the idle no longer being everyone's), then running, the
  walk still predicted exactly, all three tests in 6.5 minutes. In a practice match at the game's
  zoom the allies read in their own colours where they read blue on the blue stone. The golden
  hashes held, native and on wasm.

Not yet:
- Nobody has seen them on a real GPU, and the frame rate there isn't measured.
- An outline drawn as a line (the rim stands in); the clips' windows shared by every champion, not
  tuned one by one.
- Hand-keyed signature moments (an ultimate's own clip, the way Riot keys them), a crit's swing,
  dynamic cloth and hair: the library has none, and Meshy's credits are spent.
- The library's lantern and torch stances are a hand's, not the champions' own props' (Liora's
  lantern swings from where Meshy's grip left it); the champions' own walk under a cast is a run.

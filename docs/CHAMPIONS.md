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
  The cart rocks as it rolls and kicks back as its windup ends, when it fires.
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

Not yet:
- Nobody has seen them on a real GPU, and the frame rate there isn't measured.
- An outline (Riot's "inking") or a rim light; the clips' windows shared by every champion, not
  tuned one by one; a recall.
- A minion's flash when hit (their materials are shared a kind and team, so one can't be lit
  alone), and the cart's wheels turning.

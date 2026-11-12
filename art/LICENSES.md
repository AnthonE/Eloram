# Where the art comes from

Every file under `web/assets/` is listed here with its source and licence. A file without a line
here doesn't ship. Nothing is traced, sampled or named from Riot's games (`CLAUDE.md`).

## The icon (`crates/el-client/icon/`)

`eloram-256.png`, `eloram.ico` and `eloram.rc`, the desktop's window and program icon: drawn by
`art/tools/icon.py` from `web/favicon.svg`'s shapes (the lane between the two bases). Ours.

## The menu's backdrop (`web/assets/menu/`)

`causeway.jpg`, behind the main menu: the Causeway shot from a match of the game itself on the high
tier with the HUD off (`scripts/e2e.sh backdrop`), cropped, quietened and darkened on the left by
`art/tools/menu_backdrop.py`. Ours, made of the art listed below.

## The ground (`web/assets/ground/`)

Made by `art/tools/textures.py` from [ambientCG](https://ambientcg.com) sets, all
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/), graded and stacked:

| File | Layers (in order) | Sources |
|---|---|---|
| `albedo.jpg`, `normal.jpg`, `rha.jpg` | paving, snow, gravel, dressed slabs, cliff rock | [Bricks108A](https://ambientcg.com/view?id=Bricks108A), [Snow010A](https://ambientcg.com/view?id=Snow010A), [Gravel043](https://ambientcg.com/view?id=Gravel043), [PavingStones136](https://ambientcg.com/view?id=PavingStones136), [Rock035](https://ambientcg.com/view?id=Rock035) (CC0) |
| `runner.png` | the runner's inlay (a key pattern and slabs) | drawn by `textures.py`; ours |

## The brush (`web/assets/brush.png`)

Three frosted shrubs made with Meshy's text-to-image (`art/meshy.json`, `images`), keyed and
packed by `art/tools/sprites.py`. Ours: generated for this project on its Meshy account, under
[Meshy's terms](https://www.meshy.ai/terms-of-use).

## The models (`web/assets/models/`)

Made with Meshy's text-to-3D (`meshy-6-lite`) from our own prompts (`art/meshy.json`, `models`,
with each task's id), then stood on the origin, welded, given tangents, their textures resized
and turned to JPEG by `art/tools/models.sh` (glTF Transform). Ours: generated for this project on
its Meshy account, under [Meshy's terms](https://www.meshy.ai/terms-of-use).

| File | What it is |
|---|---|
| `turret.glb` | a hooded stone sentinel on a plinth raising a lantern of crystal fire |
| `inhibitor.glb` | a crystal floating over a ring of carved stone |
| `nexus.glb` | a great crystal held up by stone claws |
| `fountain.glb` | the platform's dais, its orb on a pillar |
| `brazier.glb` | a carved stone bowl of embers |
| `rock_a.glb` | a low snow-capped outcrop |
| `rock_b.glb` | a cluster of ice and stone spikes |
| `pillar.glb` | a broken carved column |
| `statue.glb` | a hooded guardian with a sword, on a plinth |
| `spire.glb` | a tall spire of glacial ice |
| `arch.glb` | a ruined stone arch |
| `banner.glb` | a pole with a hanging banner |

## The champions (`web/assets/models/champions/`)

Made with Meshy (`art/meshy.json`): each champion by text-to-3D (`meshy-6-lite`, in an A-pose) from
our own prompts, textured and rigged by Meshy, its run from its rigging; the other clips from
Meshy's animation library, made on one champion's rig and retargeted onto each; the props it
holds by text-to-3D (untextured), coloured in bands (`art/champions.json`). Put together by
`art/tools/champions.mjs` (glTF Transform). Ours: generated for this project on its Meshy account,
under [Meshy's terms](https://www.meshy.ai/terms-of-use).

Their second clips (the flinches, a second attack each, the stances some stand in, the throw:
`art/champions.json`, `ual`) are Quaternius's
[Universal Animation Library](https://quaternius.com/packs/universalanimationlibrary.html) and
[Universal Animation Library 2](https://quaternius.com/packs/universalanimationlibrary2.html), the
Standard sets, [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) (fetched and checked by
`art/tools/ual.py`, retargeted onto each rig by `art/tools/retarget.mjs`).

| File | The champion, and the props it holds |
|---|---|
| `brakka.glb` | Brakka: hook chain |
| `brun.glb` | Brun: forge hammer |
| `gorum.glb` | Gorum: none |
| `hollis.glb` | Hollis: mace, bell |
| `kesh.glb` | Kesh: dagger, dagger |
| `liora.glb` | Liora: lantern, halo |
| `marret.glb` | Marret: blunderbuss |
| `mira.glb` | Mira: lamp pole |
| `nimra.glb` | Nimra: none |
| `oda.glb` | Oda: walking staff, pack |
| `orrin.glb` | Orrin: censer staff |
| `pell.glb` | Pell: sword, kite shield, banner |
| `sable.glb` | Sable: lantern small |
| `sefa.glb` | Sefa: crescent blade |
| `seren.glb` | Seren: star wand |
| `talmar.glb` | Talmar: trident |
| `torek.glb` | Torek: mortar |
| `ulla.glb` | Ulla: branch staff |
| `vessa.glb` | Vessa: crossbow |
| `vire.glb` | Vire: greatsword |

## The champions' faces (`web/assets/portraits/`)

| File | What it is |
|---|---|
| `<name>.jpg`, one a champion | the game's own picture of that champion's model above, from the shoulders up, as the HUD's portrait draws it (`crates/el-client/src/portrait.rs`), taken by `scripts/e2e.sh faces`; ours, under the models' own terms. |

## The minions (`web/assets/models/minions/`)

Made with Meshy (`art/meshy.json`, `minions`) as the champions are: the melee minion's and the
caster's bodies by text-to-3D (`meshy-6-lite`, in an A-pose) from our own prompts, textured and
rigged by Meshy, each its own run; the champions' shared clips retargeted onto them; the props
the champions' (`art/champions.json`, `minions`), a band of each in its team's colour. The siege
cart by text-to-3D and textured, not rigged. Each `<name>.dusk.jpg` is its texture with the blues
turned red, by `art/tools/champions.mjs`, which puts them together. Ours: generated for this project
on its Meshy account, under [Meshy's terms](https://www.meshy.ai/terms-of-use). Their flinch is
Quaternius's (CC0), as the champions' is.

| File | The minion, and what it holds |
|---|---|
| `melee.glb`, `melee.dusk.jpg` | a squat foot soldier: sword, kite shield |
| `caster.glb`, `caster.dusk.jpg` | a hooded acolyte: walking staff |
| `super.glb`, `super.dusk.jpg` | the foot soldier's body, bigger: forge hammer |
| `siege.glb`, `siege.dusk.jpg` | a cannon on a four-wheeled cart |

## The sounds (`web/assets/audio/`)

Made by `art/tools/sounds.py` from the lines of `manifest.json` (ours: written from
`el_client_core::fx::sound`). Every file is in two formats, `<stem>.ogg` (Opus) and `<stem>.mp3`,
and `<stem>` is a sound's name, or its name and a take number (`ember.cast.1`). The physical
bodies of the sounds are recordings from five of Kenney's packs, all
[CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) (each pack's `License.txt` says so,
and the tool checks it): [Impact Sounds](https://kenney.nl/assets/impact-sounds),
[RPG Audio](https://kenney.nl/assets/rpg-audio), [Sci-fi Sounds](https://kenney.nl/assets/sci-fi-sounds),
[Casino Audio](https://kenney.nl/assets/casino-audio) and
[Interface Sounds](https://kenney.nl/assets/interface-sounds). Everything else in them (fire,
water, wind, bells, glass, starlight, the warnings, the tones, the room, the ambience) is
synthesised by the tool; ours. Each file below is a mix of those: the recordings layered,
pitched, filtered and enveloped under the synthesis.

The packs are in the repository (`art/vendor/kenney/`), as Kenney published them, each pinned by
its hash in `sounds.py`: kenney.nl's downloads now go through a signed link, and the links the
tool had pinned lead to the packs' pages. Rebuilt from them, every sound they make comes out the
same bytes as the files shipped.

| Pack | Where it came from |
|---|---|
| `kenney_sci-fi-sounds.zip`, `kenney_interface-sounds.zip`, `kenney_casino-audio.zip` | Kenney's own uploads to OpenGameArt ([sci-fi](https://opengameart.org/content/sci-fi-sounds), [interface](https://opengameart.org/content/interface-sounds), [casino](https://opengameart.org/content/54-casino-sound-effects-cards-dice-chips)): the same bytes as kenney.nl's 2023 zips |
| `kenney_rpg-audio.zip` | Kenney's own OpenGameArt upload ([50 RPG sound effects](https://opengameart.org/content/50-rpg-sound-effects), `RPGsounds_Kenney.zip`), older than kenney.nl's 2023 zip and laid out differently; the same recordings for every sound checked (11 of the 14 used; the other 3 are in sounds that need the impact pack too) |
| `kenney_impact-sounds.zip` | not in the repository yet ([kenney.nl](https://kenney.nl/assets/impact-sounds) only): the sounds made from it can't be remade until it is |

| Files | Kenney recordings in them (CC0) | Pure synthesis (ours) |
|---|---|---|
| `ember.*` | impact (punches), sci-fi (thruster, low explosion, crunch) | `cast`, `windup`, `dash`, `buff`, `ult` |
| `frost.*` | impact (glass, snow footsteps), sci-fi (crunch) | `cast`, `windup`, `dash`, `buff` |
| `tide.*` | impact (soft hits), sci-fi (low explosion) | `cast`, `launch`, `windup`, `dash`, `buff`, `ult` |
| `wind.*` | impact (soft hits, punches), RPG (cloth) | `cast`, `launch`, `windup`, `ult` |
| `dusk.*` | impact (soft hits), sci-fi (low explosion, crunch) | `cast`, `launch`, `windup`, `dash`, `buff` |
| `growth.*` | RPG (creaks, chop), impact (wood, grass footsteps, punches, soft hits) | `buff`, `ult` |
| `stone.*` | impact (mining, punches, concrete footsteps), sci-fi (crunch, low explosion) | `cast`, `windup`, `buff` |
| `bronze.*` | impact (bell, metal, wood, punches) | `cast`, `launch`, `windup`, `buff` |
| `steel.*` | RPG (knives, metal click, cloth, footsteps), impact (punches, metal), sci-fi (crunch) | `launch`, `windup` |
| `auto.*`, `minion.*`, `turret.*`, `inhibitor.fall`, `nexus.fall`, `status.*`, `shield.up`, `tag.*`, `death.*` | impact (punches, soft and generic hits, glass, metal, tin, wood, mining, snow footsteps), RPG (creaks), sci-fi (low explosion, crunch, force field) | `auto.swing`, `minion.swing`, `minion.shot`, `turret.shot`, `turret.warn`, `status.silence`, `status.slow`, `status.reveal`, `status.immune`, `tag.throw`, `tag.dash` |
| `heal`, `empowered`, `hasted`, `blink`, `respawn`, `level.up`, `relic.*` | none | all |
| `augment.proc` | Interface (tick) | |
| `ui.*`, `ping.*` | Interface (click, tick), Casino (chips, cards, the shuffle) in `ui.click`, `ui.tick`, `ui.buy`, `ui.sell`, `ui.card`, `ui.reroll` | the rest |
| `amb.wind`, `amb.fire` | none | all |

| `gold.coin`, `takedown.*` | none | all |

The music (`music.*`) is optional: it comes with its own licence when it comes.

## The voices (`web/assets/audio/voice.*`, `vo.*`)

Placeholders, made by `art/tools/voices.py` from the words in `manifest.json` (ours: the
announcer's in `el_client_core::fx::voice::says`, the champions' scripts in
`el_client_core::fx::vo`), spoken by [Piper](https://github.com/OHF-Voice/piper1-gpl) (the
program is GPL-3.0; what it speaks is not the program, and carries the voices' terms below) and
processed by the tool (pitch, pace, rooms, the families' layers: ours).

| Files | Piper voice | Its data, and licence |
|---|---|---|
| `voice.*` (the announcer) | [`en_GB-cori-high`](https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_GB/cori/high) | [LibriVox](https://librivox.org) recordings, public domain |
| `vo.*` (the champions) | [`en_GB-vctk-medium`](https://huggingface.co/rhasspy/piper-voices/tree/main/en/en_GB/vctk/medium), a speaker each (`voices.py`, `CAST`) | the CSTR VCTK Corpus (Yamagishi, Veaux, MacDonald; the Centre for Speech Technology Research, University of Edinburgh), [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/): its speakers' voices, attributed here |

`art/voices.lock.json` holds the hash of each placeholder the tool wrote. A recording that
replaces one (the same `<stem>.ogg` and `<stem>.mp3`) is never written over, and gets its own line
here, with its actor and its terms, when it comes.

## The effects' flipbooks (`web/assets/fx/`)

| File | What it is |
|---|---|
| `cloud_a.png`, `cloud_b.png` | smoke, a fireball and a flash of petals, a strip of frames each, baked by `art/tools/flipbooks.py` from its own fluid simulation and lit six ways (`docs/FX.md`, step 7); ours. The baker is after oneshot's (`AnthonE/oneshot`, `tools/flipbooks/bake.py`, MIT). |

## Made in code

The fog's texture, the floor's splat, the mist, the snow and the braziers' fire are made by the
client at startup or in its shaders (`crates/el-client/src/`): nothing to license.

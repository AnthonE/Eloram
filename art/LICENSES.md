# Where the art comes from

Every file under `web/assets/` is listed here with its source and licence. A file without a line
here doesn't ship. Nothing is traced, sampled or named from Riot's games (`CLAUDE.md`).

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

The announcer's lines (`voice.*`) and the music (`music.*`) are optional lines the tool doesn't
make; they come with their own licences when they come.

## Made in code

The fog's texture, the floor's splat, the mist, the snow and the braziers' fire are made by the
client at startup or in its shaders (`crates/el-client/src/`): nothing to license.

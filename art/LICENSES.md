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

## Made in code

The fog's texture, the floor's splat, the mist, the snow and the braziers' fire are made by the
client at startup or in its shaders (`crates/el-client/src/`): nothing to license.

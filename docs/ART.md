# The Causeway, dressed

The plan to make the map look finished: a frozen stone bridge at night, carved tiles under snow,
a drop into a blue dark on either side, frosted thickets for brush, braziers burning, sculpted
structures with their team's light in them. The pictures we're aiming at are the mood, not a source:
nothing of Riot's is traced, sampled or named (`CLAUDE.md`, "Every name is the game's own"). The UI
is not part of this pass.

## Where it stands

`el-client/src/view.rs` draws the Causeway from the map's own shapes, and nothing else:
- the floor is one plane with a 960×240 texture coloured per pixel (stone, a tint each side, green
  where the brush is, near-black off the floor);
- rock is grey cylinders the size of `map::ROCKS`' discs, brush is translucent green boxes;
- structures are `Kind` primitives in team colour (a cylinder turret, sphere inhibitor and nexus);
- one directional light without shadows and a flat ambient: no HDR, tonemapping, bloom or fog of
  the atmospheric kind;
- the fog of war is a darkened plane 5 cm over the floor, so anything that stands taller than that
  would be drawn bright over it.

What we have to work with: Bevy 0.19 is already built with glTF, PNG and KTX2/zstd, `ServeDir`
serves anything under `web/dist/` with its `.br`/`.gz` copies, and Meshy's text-to-3D, image-to-3D,
retexture and text-to-image are on this environment's proxy (479 credits).

## What makes the references read as polished

1. **Light.** Cold blue moonlight, a cool ambient, and warm fire as the accent (braziers, the
   structures' glow) with bloom on it. Soft shadows under everything.
2. **A floor with detail at three scales.** Carved paving tiles with seams and normal maps;
   a long inlaid runner down the lane's middle; snow drifted into the edges and the tile seams.
3. **Edges with depth.** The lane ends in walls and parapets that drop into a dark chasm with haze,
   not in a flat colour. That one thing is most of why the references feel like a place.
4. **Silhouettes.** Structures, rocks and brush are sculpted shapes, not primitives; brush is a
   frosted thicket you can read as cover.
5. **Small life.** Falling snow, flicker on the fires, a sway in the brush.

## The work, in order

Each step lands on its own with `scripts/ci.sh` green, and leaves the game playable if an asset
doesn't load: the current primitives stay as the fallback.

### 1. Light and the camera's image (code only)

- An HDR camera with `Tonemapping::TonyMcMapface` (or AgX; needs `tonemapping_luts`, which wants
  the ktx2 and zstd we already have), `Bloom::NATURAL`, MSAA 4× (FXAA on the low tier).
- Moonlight: the directional light blue-white (≈ 7,500 K), from high behind the camera's left so
  shapes keep their edges; one shadow cascade fitted to what the camera sees (it only ever looks
  about 40 m across), soft (PCF).
- Ambient tinted deep blue and lower; an `EnvironmentMapLight` from a CC0 snowy-night HDRI (Poly
  Haven), prefiltered to a small KTX2 cube, for the stone's and ice's highlights.
- `DistanceFog` in the chasm's blue, starting just beyond the lane so the drop fades out.
- A quality tier on the page (`?gfx=low|high`, saved): low drops shadows, bloom and MSAA.

This alone moves the picture the furthest for the least: do it first and keep a before/after.

### 2. The floor

- A floor material, `ExtendedMaterial<StandardMaterial, FloorExt>`, that blends three tiled PBR
  layers in world space: carved paving, snow, and dark rubble/ice (ambientCG, CC0; 1024² base
  colour, normal and packed occlusion/roughness, as three `texture_2d_array`s so WebGL2's 16
  sampler units hold).
- The splat mask baked at startup from the map's shapes, the way `floor_image()` is now, at a
  finer grid: snow thickening toward the floor's edge and round rock (noise from `el_sim::math::hash01`),
  rubble round the rocks' feet, a darker frozen ground under the brush, ambient occlusion along
  every wall.
- The runner: a strip down the lane's middle with an inlaid border and knotwork of our own (made
  with Meshy text-to-image or drawn procedurally, then tiled), and rings of the same inlay round
  the fountain platforms and the nexus.
- The fog of war moves into the materials: a `fog_of_war.wgsl` import samples the team's fog
  texture (bilinear, smoothstepped so the 120-unit cells don't show) at the fragment's world
  position and darkens and desaturates. Every scenery material uses it, so walls and props sit in
  the fog instead of glowing over it; the fog plane goes. Units are hidden by visibility as now.
  Only what's drawn changes: the server still decides what a team is sent.

### 3. The edges and the chasm

- The walkable outline traced at startup from `map::on_floor` (marching squares over a fine grid,
  smoothed), then extruded: a stone cliff face 8 m down, a parapet along the top with merlons and
  broken gaps placed every few metres from a small kit of pieces, snow on the caps.
- A test that the drawn edge never stands inside a walkable navigation cell, nor more than a cell
  outside one, so what looks like wall is where the champion stops.
- The chasm: a floor far below in a deep-blue gradient, fog over it, and far set dressing beyond
  the lane at the frame's top and bottom: ice spires, a ruined arch, broken bridge stubs. Seen, not
  reached.

### 4. Structures

Four glTF models, one design each side tinted by material, never by a separate model:
- **Turret**: a hooded stone sentinel on a plinth holding a lantern of team-coloured fire.
- **Inhibitor**: a crystal turning slowly over a ring of carved stone.
- **Nexus**: a great crystal in a cracked stone crown, glowing from inside.
- **Fountain**: the platform's raised dais with a glowing ring and the laser's emitter.

Made with Meshy text-to-3D from our own prompts (kept in `art/prompts.md`), remeshed to budget
(turret 6k triangles, nexus 12k), the crystal and fire as a separate primitive with an emissive
material we swap per team. Each has a destroyed state: the crystal dark, rubble at the foot.
They come in as `GltfAssetLabel::Primitive` meshes and materials into `Kit`, so the unit pool keeps
swapping `Mesh3d` handles on the slots it already has and nothing is spawned at runtime.
Their footprints are checked against the sim's discs (`OUTER_TURRET`, `NEXUS`...) in a test on the
models' bounds.

### 5. Rock and brush

- **Rock**: three or four icy outcrops (Meshy, or Poly Haven CC0 rock scans decimated), scaled to
  each of `map::ROCKS`' discs with a yaw from a hash of its index, snow on their tops by a
  world-up blend in the material.
- **Brush**: a frosted thicket of pale branching shrubs, white-blue, alpha-tested, instanced and
  scattered with blue noise inside each `map::brushes()` rectangle from a fixed hash; a vertex
  sway in the shader. It has to read as cover at a glance, so it stays denser and taller than any
  other plant on the map, with a darker ground beneath it.

### 6. Set dressing and life

- `el-client/src/scenery.rs`: a hand-placed list of props for Dawn's half, mirrored for Dusk's
  (`Disc::mirrored`): braziers along the lane's edges and at the base gates, broken pillars, banners
  in team colours, chains, ice crystals, snow piles, statues behind the platforms. A test that
  nothing stands on walkable floor unless it's flat.
- Braziers burn with a flipbook flame on a camera-facing quad and a warm point light each (a dozen
  or so, no shadows); their flicker in the shader.
- Falling snow: instanced quads in a box that follows the camera, moved in the vertex shader, so no
  system writes a transform a frame (the frame economy `ROADMAP.md` records).
- Static scenery merged by material at startup into a few meshes: the target is under 300 draw
  calls for the whole view.

### 7. Polish

Decals (cracks, scorch, drifted snow across seams), ambient occlusion baked into the splat from the
props' footprints, a look at every place the camera stops (fountain, nexus, each turret, the
relics, the brush, the edges), and the numbers below.

## Assets and where they come from

| What | Source | Licence |
|---|---|---|
| Paving, snow, rubble, rock PBR sets | ambientCG, Poly Haven | CC0 |
| Night HDRI | Poly Haven | CC0 |
| Small props (barrels, chains, banners' poles) | Kenney, Quaternius | CC0 |
| Structures, outcrops, thicket, brazier, backdrop spires | Meshy text-to-3D / image-to-3D | ours (generated) |
| Runner and inlay patterns, banner cloth | Meshy text-to-image, or procedural | ours (generated) |

Meshy's credits go furthest on what only it can give us: the four structures (two tries each),
the brazier, two outcrops, the thicket shrub and the backdrop spires, roughly 15 models. Anything
a CC0 library has comes from there.

- Sources, prompts and Meshy task ids live in `art/` (not shipped), with `art/LICENSES.md`: every
  shipped file, its source, its licence. A file with no line there doesn't ship.
- Shipped files go in `web/assets/` and `scripts/build-web.sh` copies them into `web/dist/assets/`,
  where the existing brotli/gzip step compresses them and `scripts/publish-web.sh` carries them.
- Textures as KTX2 (UASTC with zstd, mipmapped); meshes as `.glb` (Bevy's loader has no meshopt or
  Draco, so welded and simplified with `gltf-transform` and left to brotli).
- The client loads with `AssetMetaCheck::Never` (no `.meta` 404s) from a path beside the page.
- Budget: the map's art under 10 MiB brotli, fetched while the queue and champion select run, so it
  is there by the time the match starts; the match never waits for it.

## How we know it's done

- **Pictures**: `scripts/e2e.sh art` puts the camera at the fixed spots in step 7 and writes
  screenshots to `e2e/artifacts/`; they go beside the references in the PR. (SwiftShader's pictures
  are right; its frame rate means nothing.)
- **Readability**: units' rings, health bars and missiles still read against the floor at a glance,
  in and out of the fog; brush reads as cover; the edge is where the champion stops.
- **Frame rate on a real GPU**: 60 fps at 1080p on integrated graphics on `high`, and on `low` for
  older machines. Measured and written into `ROADMAP.md`, which has no GPU number yet.
- **Nothing else moves**: the wasm's size is unchanged (the art is fetched), the sim and wire are
  untouched (no `CONTENT_VERSION` bump), the vision-leak test still reads zero, and `ci.sh` is green.
- **Licences**: `art/LICENSES.md` covers every shipped file, and no file name or prompt names
  anything of Riot's.

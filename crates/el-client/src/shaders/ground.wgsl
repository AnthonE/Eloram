// The Causeway's stone (`ground.rs`): the floor, and the cliffs and parapets at its edges.
//
// Mode 0, the floor: three tiled layers (paving, snow, gravel) blended by the splat baked from the
// map's shapes, each by its height so snow fills the joints first; on the high tier, the decals
// worked out here rather than read (no texture more: a stage's 16 are near spent): cracks across
// the paving in patches, and snow drifted in streaks across the seams. Then the runner inlaid down
// the lane's middle and the rings round the bases, cut from one tile, their grooves lit with the
// team's colour. Each paving stone and each of the runner's slabs its own value and lean, and a
// broad mottle over all of it. Modes 1 and 2, the edges: cliff rock or dressed slabs, projected
// from the sides and the top (triplanar), the rock broken into blocks and the slabs weathered (the
// high tier's), snow on what faces up, darkening into the chasm's haze with depth. Every mode lies
// under the fog of war, and is filled by the night as a hemisphere (`fill.wgsl`).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#import el::fog::{fog_dark, apply_fog, map_uv, MAP_W, MAP_H}
#import el::fill::hemisphere
#import el::noise::{cell_key, unit_of, draw3, value3, value3_aa, band, RockCell, rock_cell, rock_draw}

struct Ground {
    // x: the mode; y: the layer's tile (m); z, w: where the depth's darkening starts and ends (y).
    mode: vec4<f32>,
    // x: 1 on the low tier, which reads a few textures, not a score of them.
    tier: vec4<f32>,
    // The runner: from x0 to x1 along the lane (map units), its half width, its tile's length.
    runner: vec4<f32>,
    // The rings: centre (map units), inner and outer radius; an outer radius of 0 is none.
    rings: array<vec4<f32>, 4>,
    // The rings' glow: Dawn's (the first two) and Dusk's.
    team: array<vec4<f32>, 2>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> g: Ground;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var albedo: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var albedo_s: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var normal: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var normal_s: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var rha: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var rha_s: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var splat: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var splat_s: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var runner: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var runner_s: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var fog: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(112) var fog_s: sampler;

const PAVING: i32 = 0;
const SNOW: i32 = 1;
const GRAVEL: i32 = 2;
const SLABS: i32 = 3;
const ROCK: i32 = 4;
const TAU: f32 = 6.2831853;
// The chasm's haze, linear (`look::HAZE`).
const HAZE: vec3<f32> = vec3(0.0027, 0.0050, 0.0130);

// ---- The decals: noise and cracks, in world space (m) --------------------------------------------

fn hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2(dot(p, vec2(127.1, 311.7)), dot(p, vec2(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

// Value noise, 0 to 1.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i).x;
    let b = hash2(i + vec2(1.0, 0.0)).x;
    let c = hash2(i + vec2(0.0, 1.0)).x;
    let d = hash2(i + vec2(1.0, 1.0)).x;
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// How far `p` is from the nearest edge between two cells of a Voronoi pattern (in cells): where the
// stone split.
fn crack_dist(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    var near = vec2(0.0);
    var cell = vec2(0.0);
    var best = 8.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let g = vec2(f32(x), f32(y));
            let r = g + hash2(i + g) - f;
            let d = dot(r, r);
            if d < best {
                best = d;
                near = r;
                cell = g;
            }
        }
    }
    best = 8.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let g = cell + vec2(f32(x), f32(y));
            let r = g + hash2(i + g) - f;
            let e = r - near;
            if dot(e, e) > 1e-5 {
                best = min(best, dot(0.5 * (near + r), normalize(e)));
            }
        }
    }
    return best;
}

// Cracks: thin dark lines in patches over the paving (0 to 1), fewer near the lane's middle where
// the runner is. A line a pixel or two wide (`px` m), so it's drawn as a pixel covers it (Gates'
// lesson: a line cut harder than the pixel crawls as the camera pans), and only as dark as the
// share of the pixel it covers once it's thinner than one.
fn cracks(xz: vec2<f32>, px: f32) -> f32 {
    let spots = smoothstep(0.58, 0.78, vnoise(xz * 0.13 + vec2(7.3, 1.9)));
    if spots <= 0.0 {
        return 0.0;
    }
    // A wobble, so a crack wanders rather than runs straight.
    let w = vec2(vnoise(xz * 2.7), vnoise(xz * 2.7 + vec2(4.1, 9.2))) - 0.5;
    let d = crack_dist(xz / 1.6 + w * 0.12);
    let width = 0.012 + 0.012 * vnoise(xz * 1.3);
    let aa = max(px / 1.6, 1e-5);
    let cover = clamp((width - d) / aa + 0.5, 0.0, 1.0);
    return cover * min(1.0, 2.0 * width / aa) * spots;
}

// Snow drifted by the wind in long streaks across the seams (0 to 1), in patches: everywhere at
// once, they read as scratches on the picture rather than snow on the stone.
fn drifts(xz: vec2<f32>, px: f32) -> f32 {
    // Along the wind, from the far side of the lane a little across it; the fine streaks only
    // where a pixel can carry them.
    let a = vec2(xz.x * 0.96 + xz.y * 0.28, xz.y * 0.96 - xz.x * 0.28);
    let n = vnoise(a * vec2(0.3, 2.4)) * 0.7 + mix(0.5, vnoise(a * vec2(0.9, 6.0)), band(px, 6.0)) * 0.3;
    let patches = smoothstep(0.42, 0.72, vnoise(xz * 0.06 + vec2(3.1, 5.7)));
    return smoothstep(0.64, 0.84, n) * patches;
}

// How far a paving stone's value strays and how far off level it's laid; how far the floor's broad
// mottle takes its value either way.
const STONE_SPREAD: f32 = 0.14;
const STONE_TILT: f32 = 0.09;
const MOTTLE: f32 = 0.13;

// ---- The rock face: blocks, cracks and streaks (after Gates' `ground_splat.wgsl`) ---------------
//
// Triplanar photographs on a smooth cliff read as plaster from the camera's height: one value over
// the whole face, lit as one. So the rock carries its own structure in world space: blocks a metre
// or two across, each leaning its own way so it catches the moon at its own angle and each a little
// lighter or darker; a crack along some of the boundaries between them; ice-melt streaks down the
// steep faces. The blocks are nearest-feature cells over 3D space, so a face and a ledge cut the
// same blocks and nothing has to choose a frame on the surface.

// The block size (m), how far each leans, how much its value strays, how far the lattice is warped
// (m) so a joint wanders, the share of boundaries that are cracks, their half width (cells) and
// darkness; the streaks' width and length (m) and darkness.
const ROCK_BLOCK: f32 = 1.5;
const ROCK_TILT: f32 = 0.55;
const ROCK_SPREAD: f32 = 0.26;
const ROCK_WARP: f32 = 0.45;
const ROCK_CRACKS: f32 = 0.45;
const ROCK_CRACK_W: f32 = 0.03;
const ROCK_CRACK_DARK: f32 = 0.7;
const STREAK_W: f32 = 0.32;
const STREAK_H: f32 = 3.2;
const STREAK_DARK: f32 = 0.22;

struct Face {
    // A multiplier on the rock's value, and a lean added to its normal (world space).
    shade: f32,
    tilt: vec3<f32>,
}

// The face at `wp` (m) facing `wn`, a pixel `px` metres across: whole where a block is many pixels
// across, gone to the plain photograph where it would be speckle.
fn rock_face(wp: vec3<f32>, wn: vec3<f32>, px: f32) -> Face {
    var o: Face;
    o.shade = 1.0;
    o.tilt = vec3(0.0);
    let near = 1.0 - smoothstep(0.02, 0.12, px / ROCK_BLOCK);
    if near <= 0.0 {
        return o;
    }
    let q = wp / 3.0;
    let warp = vec3(value3(q), value3(q + vec3(31.7, 11.3, 5.9)), value3(q + vec3(7.1, 23.9, 17.3))) * 2.0 - 1.0;
    let c = rock_cell((wp + warp * ROCK_WARP) / ROCK_BLOCK);
    let d = rock_draw(c, 0u, 0.08);
    var shade = 1.0 + ROCK_SPREAD * (2.0 * d.y - 1.0) * near;
    // Only some boundaries are cracks: a key for the pair, so a crack runs the boundary's length.
    let pair = cell_key(vec3(bitcast<i32>(min(c.key, c.key_b)), bitcast<i32>(max(c.key, c.key_b)), 7));
    if unit_of(pair) < ROCK_CRACKS {
        let on = smoothstep(0.5, 1.5, ROCK_CRACK_W * ROCK_BLOCK / max(px, 1e-5));
        shade *= 1.0 - ROCK_CRACK_DARK * on * near * (1.0 - smoothstep(0.0, ROCK_CRACK_W, c.edge));
    }
    // Each block leans its own way, across the face (not into it).
    let t = (rock_draw(c, 0x68e31da4u, 0.08) * 2.0 - 1.0) * ROCK_TILT * near;
    o.tilt = t - wn * dot(t, wn);
    // Streaks down what's steep: a lattice stretched tall, as dark between streaks as light in
    // them, so the face keeps its mean.
    let steep = 1.0 - smoothstep(0.35, 0.7, abs(wn.y));
    let sn = 1.0 - smoothstep(0.1, 0.4, px / STREAK_W);
    if steep * sn > 0.0 {
        let st = 0.65 * value3(vec3(wp.x / STREAK_W, wp.y / STREAK_H, wp.z / STREAK_W))
            + 0.35 * value3(vec3(wp.x / (STREAK_W * 0.44), wp.y / (STREAK_H * 0.36), wp.z / (STREAK_W * 0.44)));
        shade *= 1.0 - STREAK_DARK * steep * sn * (2.0 * smoothstep(0.38, 0.62, st) - 1.0);
    }
    o.shade = shade;
    return o;
}

// The parapet's weathering: a multiplier on its value.
fn wall_weather(wp: vec3<f32>, wn: vec3<f32>, px: f32) -> f32 {
    let fine = value3_aa(wp, 0.9, px) * 0.6 + value3_aa(wp + vec3(5.1), 2.1, px) * 0.4 - 0.5;
    let broad = value3_aa(wp + vec3(31.0), 0.25, px) - 0.5;
    var shade = 1.0 + 0.16 * (1.5 * fine + 1.2 * broad);
    // Stains down the sides, long down the wall and narrow across it.
    let side = 1.0 - smoothstep(0.35, 0.7, abs(wn.y));
    let across = wp.x * -wn.z + wp.z * wn.x;
    let st = mix(0.5, value3(vec3(across * 2.6, wp.y * 1.2, across * 0.31)), band(px, 2.6)) * 0.7
        + mix(0.5, value3(vec3(across * 7.0 + 3.0, wp.y * 2.6, 1.7)), band(px, 7.0)) * 0.3;
    shade *= 1.0 - 0.3 * side * smoothstep(0.42, 0.75, st);
    return shade;
}

struct Surf {
    col: vec3<f32>,
    // Tangent space: x along u, y along v, z out of the surface.
    n: vec3<f32>,
    rough: f32,
    height: f32,
    ao: f32,
}

fn layer(uv: vec2<f32>, k: i32) -> Surf {
    var s: Surf;
    s.col = textureSample(albedo, albedo_s, uv, k).rgb;
    s.n = textureSample(normal, normal_s, uv, k).rgb * 2.0 - 1.0;
    let r = textureSample(rha, rha_s, uv, k).rgb;
    s.rough = r.r;
    s.height = r.g;
    s.ao = r.b;
    return s;
}

fn mix_surf(a: Surf, b: Surf, t: f32) -> Surf {
    var s: Surf;
    s.col = mix(a.col, b.col, t);
    s.n = mix(a.n, b.n, t);
    s.rough = mix(a.rough, b.rough, t);
    s.height = mix(a.height, b.height, t);
    s.ao = mix(a.ao, b.ao, t);
    return s;
}

// A tangent-space normal on the floor (u along +x, v along +z) in the world.
fn floor_normal(n: vec3<f32>, strength: f32) -> vec3<f32> {
    return normalize(vec3(n.x * strength, n.z, n.y * strength));
}

struct Inlay {
    cover: f32,
    // The inlay's height (R), the border's stone or the panel's (G).
    h: f32,
    border: f32,
    // Its slope, for the normal.
    dh: vec2<f32>,
    // Its groove's light: which team's, and how much.
    glow: vec3<f32>,
    // Which of the panel's slabs (four along a tile, two across), for each its own value.
    slab: vec2<f32>,
}

// The runner along the lane and the rings round the bases, from the one tile.
fn inlay(m: vec2<f32>, low: bool) -> Inlay {
    var uv = vec2(m.x / g.runner.w, (m.y - MAP_H * 0.5) / (2.0 * g.runner.z) + 0.5);
    var cover = select(0.0, 1.0, m.x > g.runner.x && m.x < g.runner.y && abs(uv.y - 0.5) < 0.5);
    var glow = vec3(0.0);
    // Wrapping: the derivative of the ring's angle jumps at the seam; the tile repeats, so the
    // jump is a whole number of tiles and is taken back out.
    var wrap = 0.0;
    for (var i = 0; i < 4; i++) {
        let r = g.rings[i];
        let d = m - r.xy;
        let len = length(d);
        if r.w > 0.0 && len > r.z && len < r.w {
            // The band's width maps to the tile's border (0 to 0.205 across), its length to a
            // whole number of tiles round.
            let scale = (r.w - r.z) / (0.205 * 2.0 * g.runner.z);
            let n = max(1.0, round(TAU * (r.z + r.w) * 0.5 / (g.runner.w * scale)));
            uv = vec2((atan2(d.y, d.x) / TAU + 0.5) * n, (len - r.z) / (r.w - r.z) * 0.205);
            cover = 1.0;
            wrap = n;
            glow = g.team[i / 2].rgb;
        }
    }
    var dx = dpdx(uv);
    var dy = dpdy(uv);
    if wrap > 0.0 {
        dx.x -= round(dx.x / wrap) * wrap;
        dy.x -= round(dy.x / wrap) * wrap;
    }
    let t = textureSampleGrad(runner, runner_s, uv, dx, dy);
    var o: Inlay;
    o.dh = vec2(0.0);
    if !low {
        let e = vec2(1.0 / 512.0, 0.0);
        let hu = textureSampleGrad(runner, runner_s, uv + e.xy, dx, dy).r;
        let hv = textureSampleGrad(runner, runner_s, uv + e.yx, dx, dy).r;
        o.dh = vec2(hu - t.r, hv - t.r);
    }
    o.cover = cover * t.b;
    o.slab = floor(uv * vec2(4.0, 2.0));
    o.h = t.r;
    o.border = t.g;
    // The groove: the border's low cuts.
    let groove = t.g * (1.0 - smoothstep(0.35, 0.6, t.r));
    o.glow = glow * groove;
    return o;
}

struct Lit {
    col: vec3<f32>,
    n: vec3<f32>,
    rough: f32,
    ao: f32,
    glow: vec3<f32>,
    // The floor there at all (it ends at the edge).
    cover: f32,
}

fn floor_surface(wp: vec3<f32>, px: f32) -> Lit {
    let m = vec2(wp.x * 100.0 + MAP_W * 0.5, MAP_H * 0.5 - wp.z * 100.0);
    let tile = g.mode.y;
    let low = g.tier.x > 0.5;
    var pav = layer(wp.xz / tile, PAVING);
    // Each stone its own (Gates' rock blocks, on the paving): the photograph is seven stones a
    // tile, a metre each, square to the world, so a stone is a cell of the world's own grid. Each
    // a little lighter or darker and warmer or colder than the photograph, and laid a little
    // off level so it catches the moon at its own angle: from the camera's height the floor reads
    // as stones set by hand, not one tile printed over and over every seven metres.
    let stone = draw3(cell_key(vec3<i32>(vec2<i32>(floor(wp.xz * (7.0 / tile))), 3)));
    pav.col *= (1.0 + STONE_SPREAD * (2.0 * stone.x - 1.0)) * mix(vec3(0.97, 1.0, 1.04), vec3(1.04, 1.0, 0.96), stone.y);
    pav.n = vec3(pav.n.xy + (stone.yz * 2.0 - 1.0) * STONE_TILT, pav.n.z);
    // The low tier: the paving alone, the snow and gravel flat colours over it, the inlay cut from
    // the paving's own stone.
    var snow = pav;
    snow.col = vec3(0.66, 0.70, 0.78);
    snow.n = vec3(0.0, 0.0, 1.0);
    snow.rough = 0.9;
    snow.height = 0.5;
    var grav = pav;
    grav.col = pav.col * 0.55;
    var slab = pav;
    if !low {
        snow = layer(wp.xz / 4.0, SNOW);
        snow.col *= 0.8;
        grav = layer(wp.xz / 2.2, GRAVEL);
        slab = layer(wp.xz / 2.5, SLABS);
    }
    let sp = textureSample(splat, splat_s, map_uv(wp));
    // Snow settles in the joints first, then over the stones; gravel the same, less.
    var w_snow = clamp(sp.r * 2.0 - pav.height * 1.1 + (snow.height - 0.5) * 0.4, 0.0, 1.0);
    let w_grav = clamp(sp.g * 1.8 - pav.height * 0.7, 0.0, 1.0);
    var crack = 0.0;
    if !low {
        // The streaks fill the joints and the low stones first, and thicken toward the edges.
        let drift = drifts(wp.xz, px) * clamp(1.0 - pav.height * 1.2 + sp.r, 0.0, 1.0);
        w_snow = max(w_snow, drift * 0.6);
        crack = cracks(wp.xz, px) * (1.0 - w_snow) * (1.0 - w_grav * 0.5);
    }
    var s = mix_surf(pav, grav, w_grav);
    s = mix_surf(s, snow, w_snow);
    // The broad break-up (Gates' mottle): no stretch of floor is one value, at the scale of a few
    // stones and of the whole view.
    let mottle = (value3(vec3(wp.x / 5.0, 0.0, wp.z / 5.0)) - 0.5) * 0.4 + (value3(vec3(wp.x / 17.0, 7.0, wp.z / 17.0)) - 0.5) * 0.6;
    s.col *= 1.0 + MOTTLE * 2.0 * mottle;
    s.col *= 1.0 - 0.7 * crack;
    s.ao *= 1.0 - 0.5 * crack;
    s.rough = mix(s.rough, 1.0, crack);
    var o: Lit;
    o.col = s.col;
    o.n = floor_normal(s.n, 1.0);
    o.rough = s.rough;
    o.ao = s.ao * sp.b;
    o.glow = vec3(0.0);
    o.cover = sp.a;
    // The inlay over it: the border in pale dressed stone, the panel darker and bluer, the cuts
    // in shadow; snow still drifts into its low parts near the edges.
    let inl = inlay(m, low);
    if inl.cover > 0.0 {
        // Each of the panel's slabs a little lighter or darker, as the paving's stones are.
        let own = draw3(cell_key(vec3<i32>(vec2<i32>(inl.slab), 5))).x;
        let panel = 1.0 + STONE_SPREAD * (2.0 * own - 1.0) * (1.0 - inl.border);
        let base = slab.col * panel * mix(vec3(0.62, 0.70, 0.86), vec3(1.08, 1.10, 1.14), inl.border);
        var col = base * mix(0.45, 1.0, smoothstep(0.2, 0.9, inl.h));
        let n = normalize(vec3(-inl.dh.x * 18.0, 1.0, -inl.dh.y * 18.0));
        let drift = clamp(sp.r * 2.0 - inl.h * 1.4, 0.0, 1.0);
        col = mix(col, snow.col, drift);
        o.col = mix(o.col, col, inl.cover);
        o.n = normalize(mix(o.n, normalize(n + vec3(slab.n.x, 0.0, slab.n.y) * 0.3), inl.cover));
        o.rough = mix(o.rough, mix(slab.rough, 0.95, drift), inl.cover);
        o.ao = mix(o.ao, sp.b * mix(0.6, 1.0, inl.h), inl.cover);
        o.glow = inl.glow * inl.cover * (1.0 - drift);
    }
    return o;
}

// The edges: projected from x, z and above, weighted by how the surface faces (the low tier: from
// the side it faces most alone, snow a flat colour).
fn edge_surface(wp: vec3<f32>, wn: vec3<f32>, k: i32, px: f32) -> Lit {
    let tile = g.mode.y;
    let a = abs(wn);
    var w = pow(a, vec3(4.0));
    w = w / (w.x + w.y + w.z);
    var sx: Surf;
    var sz: Surf;
    var sy: Surf;
    var snow: Surf;
    if g.tier.x > 0.5 {
        let side = select(wp.xy, wp.zy, a.x > a.z);
        let one = layer(select(side, wp.xz, a.y > max(a.x, a.z)) / tile, k);
        sx = one;
        sz = one;
        sy = one;
        snow = one;
        snow.col = vec3(0.66, 0.70, 0.78);
        snow.rough = 0.9;
    } else {
        sx = layer(wp.zy / tile, k);
        sz = layer(wp.xy / tile, k);
        sy = layer(wp.xz / tile, k);
        snow = layer(wp.xz / 4.0, SNOW);
    }
    let nx = vec3(sx.n.z * sign(wn.x), sx.n.y, sx.n.x);
    let nz = vec3(sz.n.x, sz.n.y, sz.n.z * sign(wn.z));
    let ny = vec3(sy.n.x, sy.n.z * sign(wn.y), sy.n.y);
    var o: Lit;
    o.col = sx.col * w.x + sz.col * w.z + sy.col * w.y;
    o.n = normalize(nx * w.x + nz * w.z + ny * w.y + wn * 0.6);
    o.rough = sx.rough * w.x + sz.rough * w.z + sy.rough * w.y;
    o.ao = sx.ao * w.x + sz.ao * w.z + sy.ao * w.y;
    let h = sx.height * w.x + sz.height * w.z + sy.height * w.y;
    // The cliff's blocks (the high tier's); the parapet's dressed stone keeps its own courses and
    // is weathered (Gates' `weathering.wgsl`): a mottle, and stains run down its faces.
    if g.tier.x < 0.5 {
        if k == ROCK {
            let f = rock_face(wp, wn, px);
            o.col *= f.shade;
            o.ao *= mix(1.0, f.shade, 0.5);
            o.n = normalize(o.n + f.tilt);
        } else {
            o.col *= wall_weather(wp, wn, px);
        }
    }
    // Snow on what faces up.
    let top = clamp((wn.y - 0.55) * 4.0 - h * 0.6 + 0.3, 0.0, 1.0);
    o.col = mix(o.col, snow.col, top);
    o.rough = mix(o.rough, snow.rough, top);
    o.glow = vec3(0.0);
    o.cover = 1.0;
    return o;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    let wp = in.world_position.xyz;
    // The pixel's footprint (m), taken here where control flow is uniform.
    let px = max(length(dpdx(wp)), length(dpdy(wp)));
    let mode = i32(g.mode.x + 0.5);
    var s: Lit;
    if mode == 0 {
        s = floor_surface(wp, px);
    } else {
        let wn = normalize(in.world_normal) * select(-1.0, 1.0, is_front);
        s = edge_surface(wp, wn, select(SLABS, ROCK, mode == 1), px);
    }
    pbr.material.base_color = vec4(s.col, 1.0);
    pbr.material.perceptual_roughness = clamp(s.rough, 0.3, 1.0);
    pbr.material.metallic = 0.0;
    pbr.material.emissive = vec4(s.glow, 0.0);
    pbr.diffuse_occlusion = vec3(s.ao) * hemisphere(s.n);
    pbr.N = s.n;
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    // Down into the chasm, the haze takes over.
    if mode != 0 {
        let depth = smoothstep(g.mode.w, g.mode.z, wp.y);
        out.color = vec4(mix(HAZE, out.color.rgb, depth), out.color.a);
    }
    out.color = apply_fog(out.color, fog_dark(fog, fog_s, wp));
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    // Off the floor, the chasm: nothing drawn (last, so every sample above was taken everywhere).
    if s.cover < 0.5 {
        discard;
    }
    return out;
}

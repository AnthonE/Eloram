// The Causeway's stone (`ground.rs`): the floor, and the cliffs and parapets at its edges.
//
// Mode 0, the floor: three tiled layers (paving, snow, gravel) blended by the splat baked from the
// map's shapes, each by its height so snow fills the joints first; the runner inlaid down the
// lane's middle and the rings round the bases, cut from one tile, their grooves lit with the
// team's colour. Modes 1 and 2, the edges: cliff rock or dressed slabs, projected from the sides
// and the top (triplanar), snow on what faces up, darkening into the chasm's haze with depth.
// Every mode lies under the fog of war.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#import el::fog::{fog_dark, apply_fog, map_uv, MAP_W, MAP_H}

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

fn floor_surface(wp: vec3<f32>) -> Lit {
    let m = vec2(wp.x * 100.0 + MAP_W * 0.5, MAP_H * 0.5 - wp.z * 100.0);
    let tile = g.mode.y;
    let low = g.tier.x > 0.5;
    let pav = layer(wp.xz / tile, PAVING);
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
    let w_snow = clamp(sp.r * 2.0 - pav.height * 1.1 + (snow.height - 0.5) * 0.4, 0.0, 1.0);
    let w_grav = clamp(sp.g * 1.8 - pav.height * 0.7, 0.0, 1.0);
    var s = mix_surf(pav, grav, w_grav);
    s = mix_surf(s, snow, w_snow);
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
        let base = slab.col * mix(vec3(0.62, 0.70, 0.86), vec3(1.08, 1.10, 1.14), inl.border);
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
fn edge_surface(wp: vec3<f32>, wn: vec3<f32>, k: i32) -> Lit {
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
    let mode = i32(g.mode.x + 0.5);
    var s: Lit;
    if mode == 0 {
        s = floor_surface(wp);
    } else {
        let wn = normalize(in.world_normal) * select(-1.0, 1.0, is_front);
        s = edge_surface(wp, wn, select(SLABS, ROCK, mode == 1));
    }
    pbr.material.base_color = vec4(s.col, 1.0);
    pbr.material.perceptual_roughness = clamp(s.rough, 0.3, 1.0);
    pbr.material.metallic = 0.0;
    pbr.material.emissive = vec4(s.glow, 0.0);
    pbr.diffuse_occlusion = vec3(s.ao);
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

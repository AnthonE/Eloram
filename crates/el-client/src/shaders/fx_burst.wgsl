// A burst of particles (`fx/burst.rs`, `docs/FX.md`, "The effects runtime"): every particle a quad
// whose whole life is in its vertices, a closed form of its age, moved here from the time, so no
// system writes a transform a frame (as the snow is). Per vertex:
//   position: where it starts (scene metres); normal: its velocity (m/s);
//   uv: its corner; uv_b: (when it's born, how long it lives), in the shader's clock (s);
//   tangent: (its size (m), gravity (m/s²), drag (1/s), its shape); colour: linear rgb, alpha.
// Shapes: 0 a glow (a hot core in a soft halo), 1 a spark streaking along its motion, 2 a shard
// turning and glinting, 3 a puff that billows, grows and thins (smoke, dust), 4 a shockwave on the
// ground widening to its size, 5 a column of light, 6 a flash lying on the ground, 7 a star (a hit's
// flash of rays), 8 an ember that flickers and wanders as it rises, 9 a scorch left on the ground,
// 10 a fireball (in the laid-over pool: fire burning to soot; in the added pool: the heat alone, an
// energy burst), 11 a flash of petals (a hit's).
//
// A puff, a fireball and a petal flash play a flipbook (`web/assets/fx/cloud_{a,b}.png`, baked
// by `art/tools/flipbooks.py` from a fluid simulation: `docs/FX.md`, "Flipbooks"): the frame for
// its age and the next crossfaded, the smoke lit six ways by the moon and the night where the
// moon stands in the sprite's own frame, eroded from its thin parts as it ages, its heat added
// through its family's ramp. Each sprite picks a variant of its strip and a mirror by its seed.
//
// Light (the added pool) is born white-hot and cools to its colour, as fire and sparks do; smoke
// and dust (laid over) keep theirs. Every particle has a seed of its own, from where and when it
// was born, so no two puffs are the same shape and no two embers flicker together.

#import bevy_pbr::{
    mesh_view_bindings::{view, globals, lights},
    view_transformations::position_world_to_clip,
}
#import el::fog::fog_dark
#import el::display::display

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var fog: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var fog_s: sampler;
// x: 1 for the added pool (light), 0 for the laid-over one (smoke, dust, rubble, scorch).
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> pool: vec4<f32>;
// The stage's knobs (`burst.rs`, `BurstMat::knobs`; all 1 in a match): x how white-hot light is
// born, y how brightly the night lights smoke, z a puff's alpha, w a flash's and a star's brightness.
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> knobs: vec4<f32>;
// The flipbook sheet (`art/tools/flipbooks.py`): a, light from the sprite's right, left and top
// and its coverage; b, light from below, the front and behind, and its heat.
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var sheet_a: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var sheet_a_s: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var sheet_b: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var sheet_b_s: sampler;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) uv_b: vec2<f32>,
    @location(4) tangent: vec4<f32>,
    @location(5) color: vec4<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // x: its age as a share of its life; y: its shape; z: how dark the fog is where it is; w: its
    // seed (0 to 1).
    @location(2) life: vec4<f32>,
    // x: its age (s); y: how far into the crossfade to the flipbook's next frame.
    @location(3) age: vec2<f32>,
    // The flipbook's uvs on the sheet for this corner: the frame drawn (xy) and the next (zw).
    @location(4) sheet: vec4<f32>,
}

// The sheet's cut (`art/tools/flipbooks.py`; `burst.rs` holds the same): 8 × 8 cells; a strip's
// first cell, its variants and the frames in each: smoke from 0, the fireball from 32, the flash
// from 48.
const SHEET_CELLS: f32 = 8.0;

fn strip(shape: f32) -> vec3<f32> {
    if shape > 2.5 && shape < 3.5 {
        return vec3(0.0, 2.0, 16.0);
    }
    if shape > 9.5 && shape < 10.5 {
        return vec3(32.0, 2.0, 8.0);
    }
    return vec3(48.0, 4.0, 4.0);
}

fn plays_sheet(shape: f32) -> bool {
    return (shape > 2.5 && shape < 3.5) || shape > 9.5;
}

// Where a cell's corner (0 to 1 across the cell) is on the sheet.
fn cell_uv(cell: f32, corner: vec2<f32>) -> vec2<f32> {
    let row = floor(cell / SHEET_CELLS);
    let col = cell - row * SHEET_CELLS;
    return (vec2(col, row) + corner) / SHEET_CELLS;
}

// What heat `h` (0 to 1) glows in a family's colours: its edge colour as it warms, its core as it
// burns, white-hot only at the very top; brighter than linear in the heat, so a core burns
// bright, an edge smoulders, and what's barely warm gives nothing to the soot. The family's hue
// holds most of the way up: a fireball all white read as a cloud, not fire.
fn ramp(h: f32, core: vec3<f32>) -> vec3<f32> {
    let peak = max(core.r, max(core.g, core.b));
    let hue = core / max(peak, 0.001);
    let edge = hue * hue * peak * 0.35;
    var c = mix(edge, core, smoothstep(0.08, 0.6, h));
    c = mix(c, vec3(peak), smoothstep(0.88, 1.0, h) * 0.6);
    return c * (0.08 + 0.75 * h * h);
}

// What lights smoke and dust: about what the moon and the night's ambient give the stone, linear.
const NIGHT: vec3<f32> = vec3(0.30, 0.34, 0.44);

fn hash1(x: f32) -> f32 {
    return fract(sin(x * 127.1) * 43758.5453);
}

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

// Value noise, 0 to 1.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x),
               mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x), u.y);
}

fn turn(v: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2(v.x * c - v.y * s, v.x * s + v.y * c);
}

@vertex
fn vertex(v: Vertex) -> Out {
    var o: Out;
    let born = v.uv_b.x;
    let life = max(v.uv_b.y, 0.001);
    // The shader's clock wraps every hour; a particle born just before the wrap is gone after it.
    let age = globals.time - born;
    let k = age / life;
    if age < 0.0 || k >= 1.0 || v.uv_b.y <= 0.0 {
        o.clip = vec4(0.0, 0.0, -2.0, 1.0);
        return o;
    }
    let size = v.tangent.x;
    let gravity = v.tangent.y;
    let drag = v.tangent.z;
    let shape = v.tangent.w;
    // Where it is: thrown at its velocity, slowed by drag, pulled down by gravity.
    let travel = select(age, (1.0 - exp(-drag * age)) / drag, drag > 0.0001);
    var p = v.position + v.normal * travel - vec3(0.0, 0.5 * gravity * age * age, 0.0);
    let seed = hash1(born * 13.7 + v.position.x * 3.1 + v.position.z * 1.7 + v.position.y * 5.3);
    var corner = v.uv - 0.5;
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    var s = size;
    o.sheet = vec4(0.0);
    o.age = vec2(age, 0.0);
    if plays_sheet(shape) {
        // Its strip's variant and a mirror by its seed; the frame for its age, and the next.
        let st = strip(shape);
        let variant = min(floor(seed * st.y), st.y - 1.0);
        let f = k * (st.z - 1.0);
        let fa = floor(f);
        let fb = min(fa + 1.0, st.z - 1.0);
        let flip = step(0.5, fract(seed * 7.31));
        // The sheet's rows run down the image; the sprite's up is its uv's top.
        let c = vec2(mix(v.uv.x, 1.0 - v.uv.x, flip), 1.0 - v.uv.y);
        let base = st.x + variant * st.z;
        o.sheet = vec4(cell_uv(base + fa, c), cell_uv(base + fb, c));
        o.age.y = f - fa;
    }
    if shape < 0.5 {
        // Pops to its size, then shrinks away.
        s = size * (0.55 + 0.45 * smoothstep(0.0, 0.12, k)) * (1.0 - k * k);
        p += (right * corner.x + up * corner.y) * s;
    } else if shape > 6.5 && shape < 7.5 {
        // A star: opens fast, a turn of its own.
        s = size * (0.6 + 0.9 * sqrt(k));
        corner = turn(corner, seed * 3.14159);
        p += (right * corner.x + up * corner.y) * s;
    } else if shape > 7.5 && shape < 8.5 {
        // An ember: wanders side to side as it rises, and shrinks as it cools.
        s = size * (1.0 - 0.6 * k);
        p += right * sin(age * (2.0 + 2.0 * seed) + seed * 20.0) * 0.18 * k;
        p += (right * corner.x + up * corner.y) * s;
    } else if shape > 9.5 && shape < 10.5 {
        // A fireball: small as it's lit, swelling as it burns (the frames swell too).
        s = size * (0.7 + 1.5 * sqrt(k));
        p += (right * corner.x + up * corner.y) * s;
    } else if shape > 10.5 {
        // A flash of petals: opens fast, a turn of its own (the mirror is the sheet's).
        s = size * (0.7 + 0.8 * sqrt(k));
        p += (right * corner.x + up * corner.y) * s;
    } else if shape > 8.5 {
        // A scorch: flat on the ground, turned its own way, as big as it was made.
        corner = turn(corner, seed * 6.2832);
        p += vec3(corner.x, 0.0, corner.y) * size * 2.0;
    } else if shape < 1.5 {
        // Stretched along its motion on the screen, as long as it's fast.
        let vel = v.normal * exp(-drag * age) - vec3(0.0, gravity * age, 0.0);
        var along = vel - dot(vel, view.world_from_view[2].xyz) * view.world_from_view[2].xyz;
        let len = length(along);
        along = select(right, along / max(len, 0.0001), len > 0.0001);
        let side = cross(along, view.world_from_view[2].xyz);
        let stretch = size * (1.0 + clamp(len * 0.05, 0.0, 6.0)) * (1.0 - k);
        p += along * corner.y * stretch + side * corner.x * size * 0.35 * (1.0 - k);
    } else if shape < 2.5 {
        // A shard: a diamond turning.
        let a = age * (4.0 + 6.0 * fract(born * 13.7)) + born * 31.0;
        let c = cos(a);
        let sn = sin(a);
        let r = vec2(corner.x * c - corner.y * sn, corner.x * sn + corner.y * c);
        s = size * (1.0 - 0.6 * k);
        p += (right * r.x + up * r.y) * s;
    } else if shape < 3.5 {
        // A puff: its frames billow; the sprite grows a little with them, turned a little its
        // own way (its frames are upright, the smoke rising).
        s = size * (1.7 + 0.9 * sqrt(k));
        corner = turn(corner, (seed - 0.5) * 0.8);
        p += (right * corner.x + up * corner.y) * s;
    } else if shape < 4.5 || shape > 5.5 {
        // Flat on the ground: a ring widening to its size, or a flash.
        let grow = select(1.0, 0.3 + 0.7 * sqrt(k), shape < 4.5);
        p += vec3(corner.x, 0.0, corner.y) * size * 2.0 * grow;
    } else {
        // A column standing on its foot, as wide as its size, rising and thinning.
        let h = size * 8.0 * (0.4 + 0.6 * sqrt(k));
        p += right * corner.x * size * (1.0 - 0.5 * k) + vec3(0.0, (corner.y + 0.5) * h, 0.0);
    }
    o.clip = position_world_to_clip(p);
    o.uv = v.uv;
    o.color = v.color;
    o.life = vec4(k, shape, fog_dark(fog, fog_s, p), seed);
    return o;
}

// A sheet sprite's two frames, crossfaded: a (light from right, left, top; coverage) and b (from
// below, the front, behind; heat).
fn sheet_frames(sheet: vec4<f32>, blend: f32) -> array<vec4<f32>, 2> {
    let a = mix(textureSample(sheet_a, sheet_a_s, sheet.xy), textureSample(sheet_a, sheet_a_s, sheet.zw), blend);
    let b = mix(textureSample(sheet_b, sheet_b_s, sheet.xy), textureSample(sheet_b, sheet_b_s, sheet.zw), blend);
    return array(a, b);
}

// What the moon and the night give smoke, linear: about what they give the stone (`NIGHT`),
// split into the moon's share, which falls from where it stands, and the night's, which comes
// from everywhere. The lights' own numbers (lux through the camera's exposure) are far brighter
// than the stone reads: the ground's shader grades its own; this keeps to the stone's level.
const MOON_SMOKE: vec3<f32> = vec3(0.50, 0.55, 0.70);
const NIGHT_SMOKE: vec3<f32> = vec3(0.20, 0.23, 0.32);

// Smoke of albedo `tint` lit six ways by the moon and the night, where the moon stands in the
// sprite's own frame (its right and up the screen's, its front the eye's side): the side the
// moon is on is lit, the thick inside is dark, the thin edge glows through from behind.
fn lit_smoke(a: vec4<f32>, b: vec4<f32>, tint: vec3<f32>) -> vec3<f32> {
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let to_eye = view.world_from_view[2].xyz;
    var to_moon = vec3(0.0, 1.0, 0.0);
    if lights.n_directional_lights > 0u {
        to_moon = lights.directional_lights[0].direction_to_light;
    }
    let l = vec3(dot(to_moon, right), dot(to_moon, up), dot(to_moon, to_eye));
    let lit = a.r * max(l.x, 0.0) + a.g * max(-l.x, 0.0)
        + a.b * max(l.y, 0.0) + b.r * max(-l.y, 0.0)
        + b.g * max(l.z, 0.0) + b.b * max(-l.z, 0.0);
    let open = (a.r + a.g + a.b + b.r + b.g + b.b) / 6.0;
    return tint * (MOON_SMOKE * lit * 1.4 + NIGHT_SMOKE * (0.35 + 0.65 * open)) * knobs.y;
}

// Coverage eroded from its thin parts as the sprite ages past half its life (the smoke thins and
// tears rather than fading evenly), and gone by its end.
fn eroded(cover: f32, k: f32) -> f32 {
    let erode = clamp((k - 0.5) / 0.5, 0.0, 1.0) * 1.3;
    return cover * smoothstep(erode - 0.35, erode + 0.02, cover) * (1.0 - smoothstep(0.85, 1.0, k));
}

@fragment
fn fragment(o: Out) -> @location(0) vec4<f32> {
    let k = o.life.x;
    let shape = o.life.y;
    let seed = o.life.w;
    let age = o.age.x;
    let c = o.uv - 0.5;
    let d = length(c) * 2.0;
    // In the fog, dimmer (it's only drawn because the team was told of it).
    let dim = 1.0 - o.life.z * 0.6;
    if plays_sheet(shape) {
        let frames = sheet_frames(o.sheet, o.age.y);
        let fa = frames[0];
        let fb = frames[1];
        let born = smoothstep(0.0, 0.05, k);
        let col = o.color.rgb;
        if pool.x > 0.5 {
            // Light alone: a flash's petals, or an energy burst's heat, through the ramp.
            let heat = fb.a * born * (1.0 - smoothstep(0.6, 1.0, k)) * o.color.a;
            let glow = ramp(fb.a, col) * heat * knobs.w;
            return vec4(display(glow * dim), min(heat, 1.0));
        }
        // Laid over: smoke lit by the night, with the fire (a fireball's) added through it.
        let cover = eroded(fa.a, k) * born * o.color.a * knobs.z;
        let soot = select(col, vec3(0.12, 0.10, 0.09), shape > 9.5);
        let smoke = lit_smoke(fa, fb, soot) * cover;
        let heat = fb.a * born * (1.0 - smoothstep(0.5, 0.9, k)) * o.color.a;
        let glow = ramp(fb.a, col) * heat * knobs.w;
        return vec4(display((smoke + glow) * dim), min(cover, 1.0));
    }
    var a = 0.0;
    // In, quickly; out, over the rest of its life.
    var fade = smoothstep(0.0, 0.06, k) * (1.0 - smoothstep(0.55, 1.0, k));
    if shape < 0.5 {
        // A hot core in a soft halo.
        a = exp(-d * d * 10.0) * 0.85 + pow(max(1.0 - d, 0.0), 2.0) * 0.35;
    } else if shape < 1.5 {
        // A spark: its head (the end it flies toward) bright and wide, its tail thin and fading.
        let x = abs(o.uv.x - 0.5) * 2.0;
        let y = o.uv.y;
        let w = mix(0.12, 0.85, y * y);
        a = (1.0 - smoothstep(0.0, w, x)) * smoothstep(0.0, 0.6, y) * (1.0 - smoothstep(0.86, 1.0, y));
        a *= 0.55 + 0.75 * y;
    } else if shape < 2.5 {
        // A shard: a diamond lit on two facets, its edge bright, glinting as it turns.
        let m = abs(c.x) + abs(c.y);
        let inside = 1.0 - smoothstep(0.34, 0.5, m);
        let facet = select(0.55, 1.0, (c.x > 0.0) != (c.y > 0.0));
        let rim = smoothstep(0.26, 0.42, m) * inside;
        let glint = pow(max(sin(age * (9.0 + 8.0 * seed) + seed * 30.0), 0.0), 8.0);
        a = inside * facet * 0.8 + rim * 0.5 + inside * glint * 1.2;
    } else if shape < 3.5 {
        // A puff: a billowing edge, torn by noise of its own, thinning as it spreads.
        let n = vnoise(c * 4.0 + vec2(seed * 37.0, seed * 11.0 - k * 0.8)) * 0.6
            + vnoise(c * 9.0 + vec2(seed * 7.0 + k, seed * 23.0)) * 0.4;
        a = smoothstep(0.0, 0.55, 1.0 - d + (n - 0.5) * 0.7) * (0.75 - 0.35 * k) * knobs.z;
    } else if shape < 4.5 {
        // A shockwave: a sharp leading edge, a soft wake behind it toward the middle, uneven
        // round its length, thinning as it widens. The wake stays a band behind the edge: a
        // slower fall filled the ring early in its life, a pale cloud over the whole blast.
        let x = d - 0.92;
        let lead = 1.0 - smoothstep(0.0, 0.035, x);
        let wake = exp(min(x, 0.0) * (14.0 + 20.0 * k)) * 0.8;
        let ang = atan2(c.y, c.x);
        let rough = 0.7 + 0.3 * vnoise(vec2(ang * 5.0 + seed * 40.0, k * 3.0));
        a = select(lead, wake, x < 0.0) * rough * step(d, 0.97) * (1.15 - 0.4 * k);
    } else if shape < 5.5 {
        // A column: bright at its foot and its middle line, streaks rising up it.
        let x = abs(o.uv.x - 0.5) * 2.0;
        let rise = vnoise(vec2(x * 7.0 + seed * 20.0, o.uv.y * 3.0 + age * 5.0));
        a = (1.0 - smoothstep(0.0, 1.0, x)) * (0.45 + 0.75 * rise) * (1.0 - smoothstep(0.98, 1.0, o.uv.y));
        a *= smoothstep(1.0, 0.55, 1.0 - o.uv.y) * (1.0 + 1.5 * exp(-o.uv.y * 9.0));
        a += exp(-x * x * 40.0) * 0.4 * (1.0 - o.uv.y);
    } else if shape < 6.5 {
        // A flash on the ground: hot in the middle and falling away to its edge, torn by
        // turbulence boiling outward, so it reads as a burst of light, not a disc laid down (the
        // shockwave round it is what marks the edge).
        let ang = atan2(c.y, c.x);
        let boil = vnoise(vec2(ang * 2.5 + seed * 30.0, d * 4.0 - k * 5.0)) * 0.55
            + vnoise(c * 9.0 + vec2(seed * 13.0, k * 3.0)) * 0.45;
        let body = smoothstep(0.0, 0.5, 1.0 - d + (boil - 0.5) * 0.8);
        // Not so bright that it reads as a disc: the boil shows, and the fireball over it; a hot
        // middle, so a small one (a cast's) reads as light and not as a puff of smoke.
        a = (body * pow(max(1.0 - d, 0.0), 1.4) * (0.12 + 1.1 * boil) * 0.55 + exp(-d * d * 14.0) * 0.6)
            * knobs.w;
    } else if shape < 7.5 {
        // A star: four long rays and four short between them, a white core; gone fast.
        let q = abs(c) * 2.0;
        let long_rays = exp(-q.x * 28.0) * (1.0 - q.y) + exp(-q.y * 28.0) * (1.0 - q.x);
        let r = abs(turn(c, 0.785398)) * 2.0;
        let short_rays = (exp(-r.x * 34.0) * (1.0 - r.y * 1.6) + exp(-r.y * 34.0) * (1.0 - r.x * 1.6)) * 0.5;
        a = (max(long_rays, 0.0) + max(short_rays, 0.0) + exp(-d * d * 30.0)) * knobs.w;
        fade = smoothstep(0.0, 0.03, k) * (1.0 - smoothstep(0.2, 1.0, k));
    } else if shape < 8.5 {
        // An ember: a hot point that flickers.
        let flick = 0.55 + 0.45 * sin(age * (18.0 + 14.0 * seed) + seed * 50.0);
        a = (exp(-d * d * 14.0) + pow(max(1.0 - d, 0.0), 3.0) * 0.3) * flick;
        fade = smoothstep(0.0, 0.08, k) * (1.0 - smoothstep(0.4, 1.0, k));
    } else {
        // A scorch: soot in patches with a ragged edge, darkest in the middle, there a while,
        // fading slowly; the stone shows through it.
        let n = vnoise(c * 5.0 + vec2(seed * 31.0, seed * 17.0)) * 0.6
            + vnoise(c * 13.0 + vec2(seed * 7.0, seed * 3.0)) * 0.4;
        a = smoothstep(0.0, 0.45, 1.0 - d + (n - 0.5) * 0.9) * (0.25 + 0.3 * n) * (1.0 - 0.5 * d);
        fade = smoothstep(0.0, 0.05, k) * (1.0 - smoothstep(0.35, 1.0, k));
    }
    a = clamp(a, 0.0, 2.0) * fade * o.color.a;
    var col = o.color.rgb;
    if pool.x > 0.5 {
        // Born white-hot, cooling to its colour: its own brightness, unhued, early in its life.
        let hot = pow(1.0 - k, 3.0) * 0.7 * knobs.x;
        let peak = max(col.r, max(col.g, col.b));
        col = mix(col, vec3(peak), hot);
    } else {
        // Smoke and dust give no light: they're lit as the stone round them is, by the moon and
        // the night (a pale puff drawn as its own colour was a white cloud in the dark).
        col *= NIGHT * knobs.y;
    }
    let rgb = col * a * dim;
    // Premultiplied: added for light (its alpha unused), laid over for smoke (`burst.rs`).
    return vec4(display(rgb), min(a, 1.0));
}

// Embers over the braziers (`scenery.rs`), after Gates' `render/embers.rs`: short glowing streaks
// that leave the fire, rise and drift on the wind and go out. One mesh for every brazier, each ember
// a quad whose four corners share a seed point in the volume it rises through over its fire (the
// fire's place in the normal, the seed in the position: as the snow, nothing read per instance);
// moved here from that seed and the time, so no system writes a transform a frame. The fire's place
// sets its phase, so no two fires throw their sparks together.

#import bevy_pbr::{
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
}
#import el::display::display

struct Vertex {
    @location(0) position: vec3<f32>,
    // The fire this ember rises from, in the world.
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) glow: vec3<f32>,
}

// How high an ember rises before it's out (m), how wide its column spreads at the top (m), its
// streak's length and width (m), and the wind it drifts on (m/s, the snow's).
const RISE: f32 = 2.4;
const SPREAD: f32 = 0.55;
const LEN: f32 = 0.10;
const WIDTH: f32 = 0.022;
const WIND: vec2<f32> = vec2(0.35, 0.0);
// Its colour as it leaves the fire (linear, HDR: it glows without bloom) and as it cools.
const HOT: vec3<f32> = vec3(7.0, 2.6, 0.7);
const COOL: vec3<f32> = vec3(1.6, 0.25, 0.04);

fn hash(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3(12.9898, 78.233, 37.719))) * 43758.5453);
}

@vertex
fn vertex(v: Vertex) -> Out {
    let origin = v.normal;
    let seed = v.position - origin;
    let h = hash(seed + origin * 0.37);
    let h2 = hash(seed.zxy * 1.7 + 3.1);
    let t = globals.time;
    // Each ember's own life: 1.4 to 2.6 s, offset by its seed and the fire's place.
    let rate = 1.0 / (1.4 + 1.2 * h2);
    let life = fract(t * rate + h + dot(origin, vec3(0.13, 0.0, 0.29)));
    // Up, slowing as it cools; out from the fire's middle as it goes; a flutter; the wind.
    let up = RISE * (1.0 - (1.0 - life) * (1.0 - life)) * (0.6 + 0.4 * h);
    let out = seed.xz * (0.18 + SPREAD * life);
    let flutter = vec2(sin(t * (2.1 + 3.0 * h) + h * 40.0), cos(t * (1.7 + 2.0 * h2) + h2 * 30.0)) * 0.12 * life;
    let drift = WIND * life * RISE / 1.2;
    let p = origin + vec3(out.x + flutter.x + drift.x, up, out.y + flutter.y + drift.y);
    // The streak along its way (mostly up), across it as the camera sees it.
    let dir = normalize(vec3(flutter.x + drift.x + 0.001, 1.0, flutter.y + drift.y));
    let to_eye = normalize(view.world_position - p);
    let side = normalize(cross(dir, to_eye));
    let fade = 1.0 - life;
    let len = LEN * (0.5 + fade);
    // Never under 1.6 px across (`docs/FX.md`, Gates' lesson: thinner, a line crawls and
    // flickers as it moves), its light spread over the width it's drawn at.
    let px = length(view.world_position - p) * 2.0 / (view.clip_from_view[1][1] * view.viewport.w);
    let width = WIDTH * (0.6 + 0.8 * h2);
    let drawn = max(width, 1.6 * px);
    let corner = p + side * (v.uv.x - 0.5) * drawn + dir * (v.uv.y - 0.5) * len;
    var o: Out;
    o.clip = position_world_to_clip(corner);
    o.uv = v.uv;
    // Bright as it leaves, out by the top; a flicker; some embers never catch.
    let flicker = 0.65 + 0.35 * sin(t * (17.0 + 9.0 * h) + h2 * 20.0);
    let lit = step(0.18, h2) * smoothstep(0.0, 0.06, life) * fade * fade * flicker;
    o.glow = mix(COOL, HOT, fade * fade) * lit * (width / drawn);
    return o;
}

@fragment
fn fragment(o: Out) -> @location(0) vec4<f32> {
    // A soft core across the streak, tapered at its ends.
    let across = 1.0 - abs(o.uv.x - 0.5) * 2.0;
    let along = 1.0 - abs(o.uv.y - 0.5) * 2.0;
    let a = smoothstep(0.0, 0.8, across) * smoothstep(0.0, 0.5, along);
    // Added (no alpha), as the fire is.
    return vec4(display(o.glow * a), 0.0);
}

// A brazier's fire (`scenery.rs`): a quad turned to the camera, the flame drawn on it from noise
// that rises with time, added to what's behind it so it glows (and blooms).

#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::globals}

struct Flame {
    // The fire's colour (linear, bright) and a phase so no two braziers flicker together.
    color: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> flame: Flame;

fn hash2(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x),
               mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = globals.time + flame.color.a * 10.0;
    // uv: x across, y down the quad; the flame stands on its bottom edge.
    let p = vec2(in.uv.x - 0.5, 1.0 - in.uv.y);
    let n = noise(vec2(p.x * 5.0, p.y * 3.5 - t * 2.6)) * 0.6 + noise(vec2(p.x * 11.0, p.y * 7.0 - t * 4.1)) * 0.4;
    // A teardrop that narrows as it rises, torn at the top by the noise.
    let width = 0.42 * (1.0 - p.y) * (0.7 + 0.3 * sin(t * 7.0 + p.y * 4.0));
    let body = 1.0 - smoothstep(width * 0.4, width, abs(p.x + (n - 0.5) * 0.18 * p.y));
    let heat = clamp(body * (1.2 - p.y * 1.4 + n * 0.6), 0.0, 1.0);
    let col = mix(flame.color.rgb * vec3(1.0, 0.45, 0.2), flame.color.rgb, heat) * heat * heat;
    return vec4(col, heat);
}

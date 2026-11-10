// The menu's embers (`skin.rs`, `Sparks`): the Causeway's braziers throw sparks, and the still
// behind the menu can't. Three sheets of them over the whole screen, each a grid whose cells hold a
// spark or don't, rising at its own pace, swaying, flickering and dying as it climbs: nearer sheets
// bigger, faster and fewer. One draw, no texture; light that adds over the scrim.

#import bevy_ui::ui_vertex_output::UiVertexOutput
#import bevy_render::globals::Globals

@group(0) @binding(1) var<uniform> globals: Globals;

struct Sparks {
    // The sparks' colour (linear); a: how bright.
    color: vec4<f32>,
    // x: how fast they rise; y: how many (0 to 1); z: how far they sway.
    shape: vec4<f32>,
}

@group(1) @binding(0) var<uniform> sparks: Sparks;

fn hash2(p: vec2<f32>) -> vec2<f32> {
    var q = fract(vec3<f32>(p.xyx) * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yzx + 33.33);
    return fract((q.xx + q.yz) * q.zy);
}

// One sheet: `cells` across the screen's height, rising `speed` cells a second.
fn sheet(uv: vec2<f32>, cells: f32, speed: f32, seed: f32) -> f32 {
    let t = globals.time;
    var p = uv * cells;
    p.y += t * speed * sparks.shape.x;
    p.x += sin(p.y * 0.9 + seed) * sparks.shape.z;
    let cell = floor(p);
    let h = hash2(cell + vec2<f32>(seed, seed * 1.7));
    if h.x > sparks.shape.y {
        return 0.0;
    }
    let k = hash2(cell + vec2<f32>(seed + 9.0, 3.0));
    let at = (k - vec2<f32>(0.5)) * 0.7;
    let d = length(fract(p) - vec2<f32>(0.5) - at);
    let size = mix(0.018, 0.05, k.x * k.x);
    let flicker = 0.55 + 0.45 * sin(t * (2.0 + 6.0 * k.y) + h.y * 40.0);
    // A hard core in a soft light.
    let core = 1.0 - smoothstep(0.0, size, d);
    let halo = (1.0 - smoothstep(0.0, size * 3.2, d)) * 0.10;
    return (core * core + halo) * flicker;
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    // Square cells whatever the window's shape; y down the screen.
    let uv = vec2<f32>(in.uv.x * in.size.x / max(in.size.y, 1.0), in.uv.y);
    var e = sheet(uv, 9.0, 0.55, 1.0) * 1.0;
    e += sheet(uv, 15.0, 0.42, 5.0) * 0.7;
    e += sheet(uv, 24.0, 0.30, 11.0) * 0.45;
    // They die as they climb, and there are none at the very top.
    e *= smoothstep(0.02, 0.75, in.uv.y);
    let a = clamp(e * sparks.color.a, 0.0, 1.0);
    // Hotter at the core: toward white.
    let rgb = mix(sparks.color.rgb, vec3<f32>(1.0, 0.92, 0.75), clamp(e - 0.6, 0.0, 1.0));
    return vec4<f32>(rgb, a);
}

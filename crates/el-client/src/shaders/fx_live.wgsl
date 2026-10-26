// What lasts (`fx/live.rs`): a missile's head and its trail, a blast's telegraph, a field, a status
// on a unit. A quad (or a sphere, a shield's) placed where its thing is each frame; its look drawn
// here from the shape it's told and the shader's clock. Riot's rules (`docs/FX.md`): what does the
// gameplay is the brightest and sharpest part (a missile's head, a telegraph's rim); a missile shows
// which way it goes even standing still (its trail); a telegraph is exactly the hitbox's size.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, globals},
}
#import el::fog::fog_dark

struct Live {
    // The core's colour (linear) and strength.
    core: vec4<f32>,
    // The rim's colour and strength.
    edge: vec4<f32>,
    // x: the shape; y, z: two times on the shader's clock (a telegraph's start and its landing; a
    // field's next pulse and its period); w: a shape's own number.
    p: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> live: Live;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var fog: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var fog_s: sampler;

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

fn ring(d: f32, at: f32, w: f32) -> f32 {
    return 1.0 - smoothstep(0.0, w, abs(d - at));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = globals.time;
    let shape = live.p.x;
    let uv = in.uv;
    let c = uv - 0.5;
    let d = length(c) * 2.0;
    var core = 0.0;
    var edge = 0.0;
    if shape < 0.5 {
        // A missile's head: a hot point in a soft halo, flickering a little.
        let flick = 0.9 + 0.1 * sin(t * 40.0 + live.p.w * 7.0);
        core = pow(max(1.0 - d * 1.6, 0.0), 2.0) * 1.6 * flick;
        edge = pow(max(1.0 - d, 0.0), 1.5) * 0.7;
    } else if shape < 1.5 {
        // Its trail, along x from the tail (0) to the head (1), torn by noise moving back.
        let across = abs(uv.y - 0.5) * 2.0;
        let n = noise(vec2(uv.x * 6.0 + t * 9.0, uv.y * 3.0 + live.p.w));
        let along = pow(uv.x, 1.6);
        core = along * (1.0 - smoothstep(0.0, 0.45, across)) * (0.6 + 0.4 * n);
        edge = along * (1.0 - smoothstep(0.2, 1.0, across)) * 0.5 * n;
    } else if shape < 2.5 {
        // A blast's telegraph: its rim, bright and exact; inside, a fill growing toward the moment
        // it lands; and a beat quickening as it comes.
        let k = clamp((t - live.p.y) / max(live.p.z - live.p.y, 0.001), 0.0, 1.0);
        let beat = 0.75 + 0.25 * sin(t * (6.0 + 18.0 * k));
        edge = ring(d, 0.97, 0.05) * 1.4 * beat;
        core = select(0.0, 0.18 + 0.25 * k, d < k) + ring(d, k, 0.04) * 0.9 + 0.06;
        core *= step(d, 1.0);
        edge *= step(d, 1.02);
    } else if shape < 3.5 {
        // A field that lasts: its rim, a slow churn inside, and a ring going out at each pulse.
        let period = max(live.p.z, 0.05);
        let since = fract((t - live.p.y) / period);
        let swirl = noise(vec2(atan2(c.y, c.x) * 3.0 + t * 0.6, d * 4.0 - t * 0.8));
        edge = ring(d, 0.96, 0.05) * 1.1;
        core = (0.10 + 0.14 * swirl) * step(d, 1.0) + ring(d, since, 0.05) * 0.6 * (1.0 - since);
    } else if shape < 4.5 {
        // A root: a ring at the feet, a snare of vines in it.
        let a = atan2(c.y, c.x);
        let vines = smoothstep(0.6, 1.0, sin(a * 7.0 + d * 9.0 - t * 2.0));
        edge = ring(d, 0.85, 0.08) * 1.2;
        core = vines * step(0.45, d) * step(d, 0.95) * 0.8;
    } else if shape < 5.5 {
        // A column: light rising up a cylinder (a root's binding, an updraft).
        let rise = fract(uv.y * 2.0 + t * 1.5);
        core = (1.0 - uv.y) * (0.3 + 0.7 * rise) * 0.6;
        edge = (1.0 - uv.y) * 0.3;
    } else if shape < 6.5 {
        // A stun: three stars wheeling over the head.
        for (var i = 0; i < 3; i++) {
            let a = t * 4.0 + f32(i) * 2.094;
            let s = vec2(cos(a) * 0.32, sin(a) * 0.12);
            let q = length(c - s) * 2.0;
            core += pow(max(1.0 - q * 4.0, 0.0), 2.0) * 1.8;
            edge += pow(max(1.0 - q * 2.0, 0.0), 2.0) * 0.5;
        }
    } else if shape < 7.5 {
        // A shield: a rim lit where the sphere turns from the eye, lines running down it.
        let n = normalize(in.world_normal);
        let eye = normalize(view.world_position - in.world_position.xyz);
        let rim = pow(1.0 - abs(dot(n, eye)), 2.5);
        let lines = smoothstep(0.7, 1.0, sin(in.world_position.y * 22.0 + t * 5.0));
        edge = rim * 1.2;
        core = rim * lines * 0.8 + 0.04;
    } else if shape < 8.5 {
        // A mark over the head: a silence's (a circle struck through), a reveal's (an open eye).
        let circle = ring(d, 0.75, 0.12);
        var glyph = 0.0;
        if live.p.w < 0.5 {
            glyph = (1.0 - smoothstep(0.0, 0.1, abs(c.x + c.y) * 1.4)) * step(d, 0.8);
        } else {
            glyph = 1.0 - smoothstep(0.12, 0.2, d);
        }
        edge = circle * 1.2;
        core = glyph * 1.3;
    } else {
        // A glow at the feet, pulsing (a slow's chill, a haste, strength, Combo Breaker's).
        let pulse = 0.7 + 0.3 * sin(t * 5.0 + live.p.w);
        let spin = smoothstep(0.5, 1.0, sin(atan2(c.y, c.x) * 5.0 + t * (2.0 + live.p.w)));
        edge = ring(d, 0.88, 0.1) * pulse;
        core = spin * step(0.6, d) * step(d, 0.95) * 0.5;
    }
    let rgb = live.core.rgb * core * live.core.a + live.edge.rgb * edge * live.edge.a;
    let dim = 1.0 - fog_dark(fog, fog_s, in.world_position.xyz) * 0.6;
    let a = clamp(max(core * live.core.a, edge * live.edge.a), 0.0, 1.0);
    return vec4(rgb * dim, a);
}

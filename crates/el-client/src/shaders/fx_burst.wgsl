// A burst of particles (`fx/burst.rs`, `docs/FX.md`, "The effects runtime"): every particle a quad
// whose whole life is in its vertices, a closed form of its age, moved here from the time, so no
// system writes a transform a frame (as the snow is). Per vertex:
//   position: where it starts (scene metres); normal: its velocity (m/s);
//   uv: its corner; uv_b: (when it's born, how long it lives), in the shader's clock (s);
//   tangent: (its size (m), gravity (m/s²), drag (1/s), its shape); colour: linear rgb, alpha.
// Shapes: 0 a soft glow, 1 a streak along its motion, 2 a shard turning, 3 a puff that grows and
// fades (smoke, dust), 4 a ring on the ground widening to its size, 5 a column of light, 6 a flash
// lying on the ground.

#import bevy_pbr::{
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
}
#import el::fog::fog_dark

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var fog: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var fog_s: sampler;

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
    // x: its age as a share of its life; y: its shape; z: how dark the fog is where it is.
    @location(2) life: vec3<f32>,
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
    let corner = v.uv - 0.5;
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    var s = size;
    if shape < 0.5 {
        s = size * (1.0 - k * k);
        p += (right * corner.x + up * corner.y) * s;
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
        s = size * (0.5 + 1.5 * sqrt(k));
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
    o.life = vec3(k, shape, fog_dark(fog, fog_s, p));
    return o;
}

@fragment
fn fragment(o: Out) -> @location(0) vec4<f32> {
    let k = o.life.x;
    let shape = o.life.y;
    let d = length(o.uv - 0.5) * 2.0;
    var a = 0.0;
    if shape < 0.5 {
        // Hot in the middle, soft to its rim.
        a = pow(max(1.0 - d, 0.0), 1.6);
    } else if shape < 1.5 {
        let x = abs(o.uv.x - 0.5) * 2.0;
        let y = abs(o.uv.y - 0.5) * 2.0;
        a = (1.0 - smoothstep(0.2, 1.0, x)) * (1.0 - y * y);
    } else if shape < 2.5 {
        let m = abs(o.uv.x - 0.5) + abs(o.uv.y - 0.5);
        a = 1.0 - smoothstep(0.32, 0.5, m);
    } else if shape < 3.5 {
        a = (1.0 - smoothstep(0.1, 1.0, d)) * 0.6;
    } else if shape < 4.5 {
        // A bright rim, thinning as it widens.
        let w = 0.12 * (1.0 - k) + 0.03;
        a = 1.0 - smoothstep(0.0, w, abs(d - 0.92));
    } else if shape < 5.5 {
        let x = abs(o.uv.x - 0.5) * 2.0;
        a = (1.0 - smoothstep(0.0, 1.0, x)) * smoothstep(0.0, 0.15, o.uv.y) * smoothstep(1.0, 0.6, 1.0 - o.uv.y);
    } else {
        a = pow(max(1.0 - d, 0.0), 2.2) * 0.8;
    }
    // In, quickly; out, over the rest of its life.
    let fade = smoothstep(0.0, 0.06, k) * (1.0 - smoothstep(0.55, 1.0, k));
    a *= fade * o.color.a;
    // In the fog, dimmer (it's only drawn because the team was told of it).
    let dim = 1.0 - o.life.z * 0.6;
    let rgb = o.color.rgb * a * dim;
    // Premultiplied: added for glows (their alpha unused), laid over for smoke (`burst.rs`).
    return vec4(rgb, a);
}

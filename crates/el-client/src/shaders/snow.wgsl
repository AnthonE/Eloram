// Falling snow (`scenery.rs`): a few thousand flakes in a box that follows the camera, every one
// moved here in the vertex shader from where the mesh put it and the time, so no system writes a
// transform a frame. A flake is a quad (its corner in the uv) turned to face the camera.

#import bevy_pbr::{
    mesh_view_bindings::{view, globals},
    view_transformations::position_world_to_clip,
}

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) shade: f32,
}

// The box: across, up, along (m); the flake's size.
const BOX: vec3<f32> = vec3(48.0, 18.0, 36.0);
const SIZE: f32 = 0.075;

fn hash(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3(12.9898, 78.233, 37.719))) * 43758.5453);
}

@vertex
fn vertex(v: Vertex) -> Out {
    let t = globals.time;
    let h = hash(v.position);
    // Each flake its own speed and sway.
    var p = v.position;
    p.y -= t * (0.9 + 0.8 * h);
    p.x += t * 0.35 + sin(t * (0.6 + h) + h * 40.0) * 0.5;
    p.z += cos(t * (0.5 + h * 0.7) + h * 17.0) * 0.4;
    // The box's middle under the camera's look: it looks down at 56°, from 23 m (times its zoom).
    let cam = view.world_position;
    let centre = vec3(cam.x, BOX.y * 0.5, cam.z - cam.y * 0.674);
    let lo = centre - BOX * 0.5;
    p = lo + (p - lo) - floor((p - lo) / BOX) * BOX;
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let corner = (v.uv - 0.5) * SIZE * (0.7 + 0.6 * h);
    var o: Out;
    o.clip = position_world_to_clip(p + right * corner.x + up * corner.y);
    o.uv = v.uv;
    // Fainter high up, so the flakes near the lens don't flare.
    o.shade = (1.0 - smoothstep(10.0, 18.0, p.y)) * (0.5 + 0.5 * h);
    return o;
}

@fragment
fn fragment(o: Out) -> @location(0) vec4<f32> {
    let d = length(o.uv - 0.5) * 2.0;
    let a = (1.0 - smoothstep(0.2, 1.0, d)) * o.shade;
    return vec4(vec3(0.55, 0.62, 0.75) * a, a);
}

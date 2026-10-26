// The fog of war, for every material that draws the Causeway: the team's darkness (0 seen, up to
// 0.58 unseen) read at the fragment's place on the map and laid over its lit colour, dimmed, cooled
// and greyed. Only the picture: what the team isn't sent was never sent.
#define_import_path el::fog

// A scene metre is 100 map units; the map is 15,360 by 3,840 with its middle row at z = 0.
const MAP_W: f32 = 15360.0;
const MAP_H: f32 = 3840.0;

// Where a point of the scene falls on a texture laid over the whole map (top row the far side).
fn map_uv(world: vec3<f32>) -> vec2<f32> {
    let x = world.x * 100.0 + MAP_W * 0.5;
    let y = MAP_H * 0.5 - world.z * 100.0;
    return vec2(x / MAP_W, 1.0 - y / MAP_H);
}

fn fog_dark(fog: texture_2d<f32>, fog_sampler: sampler, world: vec3<f32>) -> f32 {
    // Linear filtering over 120-unit cells, then eased, so the edge is a soft rim, not squares.
    let d = textureSampleLevel(fog, fog_sampler, map_uv(world), 0.0).r;
    return smoothstep(0.0, 0.58, d);
}

fn apply_fog(color: vec4<f32>, dark: f32) -> vec4<f32> {
    let grey = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
    let night = vec3(grey) * vec3(0.30, 0.38, 0.55) * 0.55;
    return vec4(mix(color.rgb, night, dark * 0.88), color.a);
}

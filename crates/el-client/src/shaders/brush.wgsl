// The brush's thickets (`scenery.rs`, `ground.rs`'s `Brush`): the standard mesh vertex, each card's
// top bent a little by the wind (its foot stays where it grows), a sway of its own from where it
// stands and a slow gust over the whole. The fragment is everything else's (`fogged.wgsl`).

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::globals,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

// How far a card's top moves (m), at the most.
const SWAY: f32 = 0.07;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var wp = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
#ifdef VERTEX_UVS_A
    // A card's top is v 0, its foot v 1.
    let up = 1.0 - vertex.uv.y;
    let bend = up * up;
    let t = globals.time;
    let phase = dot(wp.xz, vec2(0.37, 0.23));
    let gust = 0.6 + 0.4 * sin(t * 0.35 + wp.x * 0.05);
    wp.x += bend * gust * SWAY * (0.8 * sin(t * 1.7 + phase) + 0.25 * sin(t * 4.3 + phase * 2.1));
    wp.z += bend * gust * SWAY * 0.55 * cos(t * 1.3 + phase * 1.3);
    out.uv = vertex.uv;
#endif
    out.world_position = wp;
    out.position = position_world_to_clip(wp.xyz);
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent =
        mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither =
        mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
    return out;
}

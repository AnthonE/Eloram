// A champion (`champions.rs`, `Rim`): the standard material, and a rim of its team's light where
// its surface turns away from the camera, so its silhouette reads against the night's stone
// (`docs/CHAMPIONS.md`, "Not yet": Riot's inking, as a light rather than a line).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}

struct Rim {
    // The rim's colour, linear; w: how bright it is at the very edge.
    color: vec4<f32>,
    // x: how tight it hugs the edge (the fresnel's power); y: how much of it is kept on what
    // faces down (a rim from below reads as a glow on the floor's side).
    shape: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> rim: Rim;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    pbr.material.base_color = alpha_discard(pbr.material, pbr.material.base_color);
    let facing = clamp(dot(normalize(pbr.N), normalize(pbr.V)), 0.0, 1.0);
    let up = mix(rim.shape.y, 1.0, smoothstep(-0.4, 0.3, pbr.N.y));
    let edge = pow(1.0 - facing, rim.shape.x) * rim.color.w * up;
    pbr.material.emissive = vec4(pbr.material.emissive.rgb + rim.color.rgb * edge, pbr.material.emissive.a);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}

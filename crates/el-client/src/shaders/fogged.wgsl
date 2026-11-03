// Everything that stands on the Causeway (`ground.rs`, `Fogged`): the standard material, then the
// fog of war over it. Optionally: the crystal part of a model (a band of its height: a turret's
// lantern, the inhibitor's and the nexus's crystals) glowing in the team's colour, snow
// on what faces up, and foliage lit as the ground it grows from rather than as the flat cards it's
// drawn on.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}
#import el::fog::{fog_dark, apply_fog}

// The chasm's haze, linear (`look::HAZE`), and the depths it takes over between (as the cliffs').
const HAZE: vec3<f32> = vec3(0.0027, 0.0050, 0.0130);
const DEEP: f32 = -14.0;

struct Fogged {
    // x: the crystal's glow (0: none); y: snow on top; z: foliage (1) or not (0); w: the height
    // (m) the crystal starts at.
    look: vec4<f32>,
    // The glow's colour, linear; w: the height (m) the crystal ends at.
    tint: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> f: Fogged;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var fog: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var fog_s: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    pbr.material.base_color = alpha_discard(pbr.material, pbr.material.base_color);
    var base = pbr.material.base_color.rgb;
    if f.look.x > 0.0 {
        let y = in.world_position.y;
        let mask = smoothstep(f.look.w, f.look.w + 0.3, y) * (1.0 - smoothstep(f.tint.w - 0.3, f.tint.w, y));
        let lum = dot(base, vec3(0.2126, 0.7152, 0.0722));
        base = mix(base, f.tint.rgb * (0.4 + lum), mask);
        pbr.material.emissive = vec4(f.tint.rgb * mask * f.look.x * (0.4 + 1.2 * lum), 0.0);
    }
    if f.look.y > 0.0 {
        let up = smoothstep(0.55, 0.85, normalize(in.world_normal).y) * f.look.y;
        base = mix(base, vec3(0.78, 0.84, 0.95), up);
    }
    pbr.material.base_color = vec4(base, pbr.material.base_color.a);
    if f.look.z > 0.0 {
        pbr.N = normalize(mix(pbr.N, vec3(0.0, 1.0, 0.0), 0.75));
    }
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    // What stands down in the chasm (the spires, the ice, the ruined arches) sinks into its haze
    // as the cliffs do, so the drop reads as deep and the eye stays on the lane.
    let depth = smoothstep(DEEP, 0.0, in.world_position.y);
    out.color = vec4(mix(HAZE, out.color.rgb, depth), out.color.a);
    out.color = apply_fog(out.color, fog_dark(fog, fog_s, in.world_position.xyz));
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}

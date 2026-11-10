// Everything that stands on the Causeway (`ground.rs`, `Fogged`): the standard material, then the
// fog of war over it. Optionally: the crystal part of a model (a band of its height: a turret's
// lantern, the inhibitor's and the nexus's crystals) glowing in the team's colour, snow
// on what faces up, foliage lit as the ground it grows from rather than as the flat cards it's
// drawn on, weathering (a mottle, grime at the foot, frost on flat tops) and rock broken into
// blocks. The night's fill on all of it is a hemisphere (`fill.wgsl`).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}
#import el::fog::{fog_dark, apply_fog}
#import el::fill::hemisphere
#import el::noise::{value3, value3_aa, rock_cell, rock_draw}

// The chasm's haze, linear (`look::HAZE`), and the depths it takes over between (as the cliffs').
const HAZE: vec3<f32> = vec3(0.0027, 0.0050, 0.0130);
const DEEP: f32 = -14.0;

struct Fogged {
    // x: the crystal's glow (0: none); y: snow on top; z: foliage (1) or not (0); w: the height
    // (m) the crystal starts at.
    look: vec4<f32>,
    // The glow's colour, linear; w: the height (m) the crystal ends at.
    tint: vec4<f32>,
    // Weathering: x the mottle's depth, y grime at the foot, z frost on the flat tops, w rock's
    // facets (how far each block leans).
    wear: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> f: Fogged;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var fog: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var fog_s: sampler;

// A rock's block (m).
const FACET: f32 = 0.8;
// The colour of snow and frost on stone, linear.
const SNOW: vec3<f32> = vec3(0.78, 0.84, 0.95);

// Weathering on what the models' atlases paint the same everywhere (after Gates' and oneshot's
// `weathering.wgsl`): in world space, so two statues of one model are never one value. A mottle
// at a metre or so and at five; a ragged band of grime where a thing meets the floor, so it
// stands on the stone rather than over it; frost settled on its flat tops in patches (flat only:
// a chamfer a pixel wide at range would sparkle). Each term fades to its mean where a pixel can't
// carry it.
fn weathered(c0: vec3<f32>, p: vec3<f32>, ng: vec3<f32>, px: f32, crystal: f32) -> vec3<f32> {
    var c = c0;
    let fine = value3_aa(p, 0.8, px) * 0.6 + value3_aa(p + vec3(5.1), 1.9, px) * 0.4 - 0.5;
    let broad = value3_aa(p + vec3(31.0), 0.2, px) - 0.5;
    c *= max(1.0 + f.wear.x * (1.5 * fine + 1.2 * broad), 0.0);
    let stone = 1.0 - crystal;
    let ragged = 0.3 * (0.55 + 0.9 * value3_aa(p + vec3(47.0), 2.2, px));
    let grime = (1.0 - smoothstep(0.0, ragged, p.y)) * smoothstep(-0.3, -0.05, p.y) * f.wear.y * stone;
    c *= 1.0 - clamp(grime, 0.0, 1.0);
    let up = smoothstep(0.8, 0.95, ng.y);
    let patches = smoothstep(0.4, 0.62, value3_aa(p + vec3(13.0), 0.9, px) * 0.65 + value3_aa(p, 2.6, px) * 0.35);
    let frost = clamp(up * patches * f.wear.z * stone, 0.0, 1.0);
    let grain = 0.8 + 0.4 * value3_aa(p, 11.0, px);
    return mix(c, SNOW * grain, frost);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    let wp = in.world_position.xyz;
    // The pixel's footprint (m), where control flow is uniform (before the brush's cut-out).
    let px = max(length(dpdx(wp)), length(dpdy(wp)));
    pbr.material.base_color = alpha_discard(pbr.material, pbr.material.base_color);
    var base = pbr.material.base_color.rgb;
    let y = wp.y;
    var mask = 0.0;
    if f.look.x > 0.0 {
        mask = smoothstep(f.look.w, f.look.w + 0.3, y) * (1.0 - smoothstep(f.tint.w - 0.3, f.tint.w, y));
    }
    if dot(f.wear.xyz, vec3(1.0)) > 0.0 {
        let ng = normalize(in.world_normal) * select(-1.0, 1.0, is_front);
        base = weathered(base, wp, ng, px, mask);
    }
    // Rock broken into blocks, as the cliffs are (`ground.wgsl`, after Gates' rock face): each
    // leans its own way, so it catches the moon at its own angle, and is a little lighter or
    // darker; less on what faces up, where the snow lies smooth over them.
    if f.wear.w > 0.0 {
        let near = 1.0 - smoothstep(0.02, 0.1, px / FACET);
        let q = wp / 2.0;
        let warp = vec3(value3(q), value3(q + vec3(31.7, 11.3, 5.9)), value3(q + vec3(7.1, 23.9, 17.3))) * 2.0 - 1.0;
        let c = rock_cell((wp + warp * 0.25) / FACET);
        let lay = 1.0 - 0.6 * smoothstep(0.6, 0.9, pbr.N.y);
        let t = (rock_draw(c, 0x68e31da4u, 0.1) * 2.0 - 1.0) * f.wear.w * near * lay;
        pbr.N = normalize(pbr.N + t - pbr.N * dot(t, pbr.N));
        base *= 1.0 + 0.22 * (2.0 * rock_draw(c, 0u, 0.1).y - 1.0) * near;
    }
    if f.look.x > 0.0 {
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
    pbr.diffuse_occlusion *= hemisphere(pbr.N);
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

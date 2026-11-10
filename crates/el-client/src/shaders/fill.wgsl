// The night's fill as a hemisphere (after Gates' and oneshot's `fill.rs`): what faces up is filled
// by the sky, what faces down by the snow and stone the moon lights. Bevy's ambient light can't be
// one (it never reads the normal: the underside of a statue's arm was filled as the floor was, and
// every shape in the moon's shadow went flat). Gates and oneshot hand Bevy a baked cube as an
// environment map instead; here that cube's two textures would take the ground's material past the
// sixteen a stage may sample, so the ambient (`look.rs`) stays as it was, and the materials that
// draw the Causeway (the ground, and what stands on it) scale it by the normal through their
// diffuse occlusion, which only ever multiplies the indirect light. Champions keep the flat fill:
// their portraits are drawn with the same material under a light of their own.
#define_import_path el::fill

// Shares of the ambient. On what faces straight up, the night sky's deep blue, as the old flat
// fill was (the floor faces up and keeps the light it had). On what faces straight down, the snow
// and stone the moon lights: the brighter half on a moonlit snowfield, and paler, the moon's own
// colour rather than the night's, so a statue's side in the moon's shadow keeps its form instead of
// going to navy.
const SKY: vec3<f32> = vec3(1.0, 1.0, 1.05);
const BOUNCE: vec3<f32> = vec3(1.9, 1.6, 1.1);

// The fill on a surface facing `n` (world space), as a share of the ambient (Gates' blend: the sky's
// share is 0.5 + 0.5 n.y).
fn hemisphere(n: vec3<f32>) -> vec3<f32> {
    return mix(BOUNCE, SKY, 0.5 + 0.5 * clamp(n.y, -1.0, 1.0));
}

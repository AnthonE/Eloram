// A gauge (`skin.rs`): the health and the mana on the HUD, drawn whole by one fragment shader. A
// well sunk into the panel (dark at its top, as a thing cut in is); the fill a glass of colour, lit
// along its upper half, a bright edge where it ends, a slow light moving through it; a mark each
// tick (a hundred health) and a heavier one each tenth (a thousand), over the fill only; what was
// just lost left behind it a moment in another colour and then let go (the CPU moves it); a
// shield as a pale stretch past the fill; and the well's edge beating red while it's low.

#import bevy_ui::ui_vertex_output::UiVertexOutput
#import bevy_render::globals::Globals

@group(0) @binding(1) var<uniform> globals: Globals;

struct Gauge {
    // The fill at its foot and its top (linear).
    lo: vec4<f32>,
    hi: vec4<f32>,
    // What was just lost.
    lost: vec4<f32>,
    // The well.
    back: vec4<f32>,
    // x: the fill (0 to 1); y: where what was lost ends; z: a tick's share of the whole (0: none);
    // w: the shield's end.
    amount: vec4<f32>,
    // x: the low beat (0 to 1); y: the light's speed through it; z: every which tick is heavy.
    live: vec4<f32>,
}

@group(1) @binding(0) var<uniform> gauge: Gauge;

fn rounded(p: vec2<f32>, h: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(h.x, h.y));
    let q = abs(p) - h + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let h = in.size * 0.5;
    let p = (in.uv - vec2<f32>(0.5)) * in.size;
    let d = rounded(p, h, in.border_radius.x);
    let cover = clamp(0.5 - d, 0.0, 1.0);
    if cover <= 0.0 {
        discard;
    }
    let inside = -d;
    let u = in.uv.x;
    let v = in.uv.y;
    let px = 1.0 / max(in.size.x, 1.0);

    // The well: darker at its top, a beat of red at its edge while the fill is low.
    var color = gauge.back.rgb * (0.55 + 0.45 * smoothstep(0.0, 0.7, v));
    let beat = gauge.live.x * (0.5 + 0.5 * sin(globals.time * 6.0));
    color += vec3<f32>(0.55, 0.03, 0.02) * beat * exp(-inside / 5.0);

    let fill = gauge.amount.x;
    // What was just lost, fading toward its far end.
    let lost = step(u, gauge.amount.y) * step(fill, u);
    color = mix(color, gauge.lost.rgb * (0.75 + 0.25 * (1.0 - v)), lost * gauge.lost.a);
    // The shield, past the fill.
    let shield = step(u, gauge.amount.w) * step(fill, u);
    color = mix(color, vec3<f32>(0.80, 0.84, 0.88) * (1.0 - 0.35 * v), shield);

    // The fill: a glass of its colour.
    let on = 1.0 - smoothstep(fill - px * 0.5, fill + px * 0.5, u);
    var glass = mix(gauge.hi.rgb, gauge.lo.rgb, smoothstep(0.0, 1.0, v));
    // Lit along its upper half, the light falling off under a hard line.
    let gloss = (1.0 - smoothstep(0.38, 0.52, v)) * (0.20 + 0.16 * (1.0 - v / 0.5));
    glass += vec3<f32>(gloss);
    // A dark foot.
    glass *= 1.0 - 0.28 * smoothstep(0.62, 1.0, v);
    // A slow light through it.
    let wave = sin((u * in.size.x * 0.045) - globals.time * gauge.live.y) * 0.5 + 0.5;
    glass += gauge.hi.rgb * 0.10 * wave * wave * (1.0 - v);
    // A bright edge where it ends (not at the full end).
    let tip = (1.0 - smoothstep(0.0, 2.5 * px, fill - u)) * step(u, fill) * step(fill, 0.995);
    glass += vec3<f32>(0.55) * tip;
    // The ticks, over the fill: a hair each, a heavier and taller one each `live.z`.
    if gauge.amount.z > 0.0 && gauge.amount.z * in.size.x > 2.5 {
        let k = u / gauge.amount.z;
        let near = abs(k - round(k)) * gauge.amount.z * in.size.x;
        let which = round(k);
        if which > 0.5 {
            let heavy = step(abs(which / max(gauge.live.z, 1.0) - round(which / max(gauge.live.z, 1.0))), 0.01);
            let half = mix(0.5, 1.0, heavy);
            let reach = mix(0.55, 1.0, heavy);
            let line = (1.0 - smoothstep(half - 0.5, half + 0.5, near)) * step(v, reach);
            glass = mix(glass, vec3<f32>(0.0), line * mix(0.55, 0.85, heavy));
        }
    }
    color = mix(color, glass, on);

    // The edge: a dark line round it all, a faint light under its foot (cut into the panel).
    let rim = 1.0 - smoothstep(0.5, 1.5, inside);
    color = mix(color, vec3<f32>(0.0), rim * 0.85);
    // Shade under the well's top edge, over everything.
    color *= 1.0 - 0.35 * (1.0 - smoothstep(0.0, 0.22, v)) * (1.0 - on * 0.6);
    return vec4<f32>(color, gauge.back.a * cover);
}

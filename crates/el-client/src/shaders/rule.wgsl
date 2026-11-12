// The UI's rule (`skin.rs`): the line between a panel's parts. A hairline of bronze that fades to
// nothing at both ends, a second fainter one under it, and a gem set where they'd cross the
// middle: a diamond with its upper faces lit and a spark in it, a short bar each side of it.
// One draw; nothing here reads a texture.

#import bevy_ui::ui_vertex_output::UiVertexOutput

struct Rule {
    // The metal lit, and in shade (linear).
    hi: vec4<f32>,
    lo: vec4<f32>,
    // The gem's light.
    gem: vec4<f32>,
    // x: the gem's half-width (px; 0: none); y: how far from each end the line has faded in
    // (a share of the half-length); z: the line's strength.
    shape: vec4<f32>,
}

@group(1) @binding(0) var<uniform> rule: Rule;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let p = (in.uv - vec2<f32>(0.5)) * in.size;
    let half = in.size.x * 0.5;
    let s = rule.shape.x;
    // The line: bright in the middle, gone at the ends, parted round the gem.
    let along = 1.0 - abs(p.x) / max(half, 1.0);
    let fade = smoothstep(0.0, max(rule.shape.y, 0.01), along);
    let gap = smoothstep(s + 3.0, s + 7.0, abs(p.x));
    let line = (1.0 - smoothstep(0.25, 0.9, abs(p.y + 0.5))) * fade * gap * rule.shape.z;
    let under = (1.0 - smoothstep(0.25, 0.9, abs(p.y - 1.5))) * fade * gap * rule.shape.z * 0.45;
    var rgb = rule.hi.rgb * line + rule.lo.rgb * under;
    var a = max(line * rule.hi.a, under);
    if s > 0.0 {
        // A bar each side of the gem, heavier than the line.
        let bar_at = abs(abs(p.x) - (s + 10.0));
        let bar = (1.0 - smoothstep(4.0, 6.0, bar_at)) * (1.0 - smoothstep(0.8, 1.6, abs(p.y)));
        rgb = mix(rgb, rule.hi.rgb, bar);
        a = max(a, bar);
        // The gem.
        let m = abs(p.x) + abs(p.y);
        let body = 1.0 - smoothstep(s - 0.5, s + 0.5, m);
        let face = select(rule.lo.rgb, rule.hi.rgb, p.y < 0.0) * mix(0.7, 1.0, step(p.x, 0.0));
        var gem = mix(face, rule.gem.rgb, (1.0 - smoothstep(0.0, s * 0.6, m)) * rule.gem.a);
        gem = gem * (1.0 - 0.55 * smoothstep(s - 1.6, s - 0.4, m));
        let dark = (1.0 - smoothstep(s + 0.5, s + 1.6, m)) * 0.75;
        rgb = mix(rgb * (1.0 - dark), gem, body);
        a = max(a, max(body, dark));
    }
    return vec4<f32>(rgb, a);
}

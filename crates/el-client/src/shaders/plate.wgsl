// The UI's plate (`skin.rs`): what a panel, a button, a slot or a card is made of. One fragment
// shader draws the whole of a node: a slab of dark slate (a gradient, a grain, a shade toward its
// edges, as a thing with thickness has) inside a rim of worked bronze lit from above (a ridge on
// a wide rim: its outer face catching the light along the top, its inner along the bottom; a
// plain lit edge on a thin one), its corners cut at 45° or rounded as the node's are. Over it,
// for what's alive: a glow inside the rim that can breathe, and a sheen that crosses it.
//
// One draw where a panel was a shadow, a border and a gradient; nothing here reads a texture.

#import bevy_ui::ui_vertex_output::UiVertexOutput
#import bevy_render::globals::Globals

@group(0) @binding(1) var<uniform> globals: Globals;

struct Plate {
    // The slab, top to bottom (linear, straight alpha).
    top: vec4<f32>,
    bottom: vec4<f32>,
    // The rim where the light falls on it, and where it doesn't.
    rim_hi: vec4<f32>,
    rim_lo: vec4<f32>,
    // The glow inside the rim; a: how much of it.
    glow: vec4<f32>,
    // Each corner's cut (px): top left, top right, bottom right, bottom left. 0: the node's radius.
    cuts: vec4<f32>,
    // x: the rim's width (px); y: the grain; z: the shade toward the edges; w: how far the glow
    // reaches in (px).
    shape: vec4<f32>,
    // x: the sheen's strength; y: its period (s); z: the glow's breath (0: steady); w: a line of
    // light along the top's inside, its strength.
    live: vec4<f32>,
}

@group(1) @binding(0) var<uniform> plate: Plate;

fn corner(p: vec2<f32>, v: vec4<f32>) -> f32 {
    if p.x < 0.0 {
        if p.y < 0.0 { return v.x; }
        return v.w;
    }
    if p.y < 0.0 { return v.y; }
    return v.z;
}

// The distance to the node's edge (px, negative inside) and the way out from it: (d, n.x, n.y).
// Normals worked out here, not from screen derivatives: those turn over between the two GPU APIs.
fn edge(p: vec2<f32>, h: vec2<f32>, cut: f32, radius: f32) -> vec3<f32> {
    let s = sign(p + vec2<f32>(1e-5));
    if cut > 0.0 {
        let q = abs(p) - h;
        let c = (q.x + q.y + cut) * 0.70710678;
        if c > q.x && c > q.y { return vec3<f32>(c, s * 0.70710678); }
        if q.x > q.y { return vec3<f32>(q.x, s.x, 0.0); }
        return vec3<f32>(q.y, 0.0, s.y);
    }
    let r = min(radius, min(h.x, h.y));
    let q = abs(p) - h + vec2<f32>(r);
    if q.x > 0.0 && q.y > 0.0 {
        let l = length(q);
        return vec3<f32>(l - r, s * q / max(l, 1e-4));
    }
    if q.x > q.y { return vec3<f32>(q.x - r, s.x, 0.0); }
    return vec3<f32>(q.y - r, 0.0, s.y);
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let h = in.size * 0.5;
    let p = (in.uv - vec2<f32>(0.5)) * in.size;
    let e = edge(p, h, corner(p, plate.cuts), corner(p, in.border_radius));
    let d = e.x;
    let n = e.yz;
    let cover = clamp(0.5 - d, 0.0, 1.0);
    if cover <= 0.0 {
        discard;
    }
    let inside = -d;
    let w = plate.shape.x;
    // The light comes from above and a little to the left.
    let light = vec2<f32>(-0.31623, -0.94868);
    let facing = dot(n, light);

    // The slab.
    // Mixed with the alpha in: a clear white to a clear black mustn't pass through a grey haze.
    let slab_a = mix(plate.top.a, plate.bottom.a, in.uv.y);
    let slab_rgb = mix(plate.top.rgb * plate.top.a, plate.bottom.rgb * plate.bottom.a, in.uv.y);
    var slab = vec4<f32>(slab_rgb / max(slab_a, 1e-4), slab_a);
    let deep = max(inside - w, 0.0);
    slab = vec4<f32>(slab.rgb * (1.0 - plate.shape.z * exp(-deep / 16.0)), slab.a);
    // A line of light under the top's rim: the slab's own edge.
    let lip = (1.0 - smoothstep(0.5, 2.0, deep)) * max(-n.y, 0.0) * plate.live.w;
    slab = vec4<f32>(slab.rgb + plate.rim_hi.rgb * lip, max(slab.a, lip * plate.rim_hi.a));
    // The glow inside the rim.
    let breath = 1.0 + plate.live.z * (0.5 * sin(globals.time * 3.2) - 0.5);
    let g = plate.glow.a * breath * exp(-deep / max(plate.shape.w, 1.0));
    slab = vec4<f32>(slab.rgb + plate.glow.rgb * g, max(slab.a, min(g, 1.0)));

    // The rim.
    var color = slab;
    if w > 0.0 {
        let t = clamp(inside / w, 0.0, 1.0);
        var lit: f32;
        if w >= 3.0 {
            // A ridge: the outer face looks out, the inner in.
            let face = select(-1.0, 1.0, t < 0.5);
            lit = facing * face * 0.5 + 0.5;
            // The crest catches a little more.
            lit += 0.25 * (1.0 - smoothstep(0.0, 0.22, abs(t - 0.5)));
        } else {
            lit = facing * 0.5 + 0.5;
        }
        var rim = mix(plate.rim_lo, plate.rim_hi, clamp(lit, 0.0, 1.0));
        // A dark hair where the rim meets the world, and where it meets the slab.
        let hair = (1.0 - smoothstep(0.0, 1.0, inside)) * 0.55
            + (1.0 - smoothstep(0.0, 1.0, abs(inside - w))) * 0.35 * step(3.0, w);
        rim = vec4<f32>(rim.rgb * (1.0 - hair), rim.a);
        let on_rim = 1.0 - smoothstep(w - 0.5, w + 0.5, inside);
        color = mix(slab, rim, on_rim);
    }

    // The grain: the slab's, a little of it on the rim.
    let grain = (hash(floor(in.position.xy)) - 0.5) * plate.shape.y;
    color = vec4<f32>(color.rgb * (1.0 + grain) + vec3<f32>(grain * 0.012), color.a);

    // The sheen: a band of light that crosses, leaning.
    if plate.live.x > 0.0 {
        let u = in.uv.x + (in.uv.y - 0.5) * 0.45 * in.size.y / max(in.size.x, 1.0);
        // Across in a third of its period, then gone till the next.
        let at = mix(-0.4, 3.0, fract(globals.time / max(plate.live.y, 0.1)));
        let band = exp(-pow((u - at) / 0.07, 2.0));
        color = vec4<f32>(color.rgb + plate.rim_hi.rgb * band * plate.live.x, max(color.a, band * plate.live.x * 0.6));
    }
    return vec4<f32>(color.rgb, color.a * cover);
}

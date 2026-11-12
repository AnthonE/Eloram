// The UI's plate (`skin.rs`): what a panel, a button, a slot or a card is made of. One fragment
// shader draws the whole of a node: a slab of dark slate (a gradient, a grain, a shade toward its
// edges, as a thing with thickness has) inside a rim of worked bronze lit from above (a ridge on
// a wide rim: its outer face catching the light along the top, its inner along the bottom; a
// plain lit edge on a thin one), its corners cut at 45° or rounded as the node's are. Over it,
// for what's alive: a glow inside the rim that can breathe, and a sheen that crosses it. And its
// dressings, each off at 0: a hairline inlaid inside the rim with a bracket at each corner, veins
// through the stone, a light falling in from the top, a band across the head, a gem in the top
// rim, a light that runs round the rim, and metal that turns through the colours.
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
    // The dressing. x: a hairline inlaid this far inside the rim (px; 0: none); y: its strength,
    // and its brackets' at the corners; z: a light falling into the slab from the top's middle;
    // w: the stone's veins.
    dress: vec4<f32>,
    // x: a gem hung from the middle of the top rim (the edge cuts its upper half), its
    // half-width (px; 0: none); y: a light that
    // runs round the rim, its strength; z: its lap (s); w: the brackets' length (px).
    orn: vec4<f32>,
    // x: how far the metal and its light turn through the colours as they go round (a prismatic
    // thing; 0: none); y: a band across the head, its height (px; 0: none).
    more: vec4<f32>,
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

fn hash2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.3183, 0.3678));
    let r = q + dot(q, q.yx + 19.19);
    return fract((r.x + r.y) * r.x);
}

fn value(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2<f32>(1.0, 0.0)), u.x),
               mix(hash2(i + vec2<f32>(0.0, 1.0)), hash2(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

// A colour turned round the grey axis by `a` (radians).
fn turned(c: vec3<f32>, a: f32) -> vec3<f32> {
    let k = vec3<f32>(0.57735);
    let ca = cos(a);
    return c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca);
}

// The stone's veins: a soft cloud, and thin lines where a second one crosses its middle.
fn veins(p: vec2<f32>) -> f32 {
    let a = value(p / 70.0) * 0.6 + value(p / 23.0 + vec2<f32>(7.3, 1.7)) * 0.4;
    let b = value(p / 38.0 + vec2<f32>(a * 2.2, -a * 1.6));
    let line = 1.0 - smoothstep(0.0, 0.035, abs(b - 0.5));
    return (a - 0.5) + line * 0.35;
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
    // The metal: its own colours, or turning through the others as it goes round.
    var rim_hi = plate.rim_hi;
    var rim_lo = plate.rim_lo;
    var glow_c = plate.glow.rgb;
    if plate.more.x > 0.0 {
        let a = (globals.time * 0.7 + (in.uv.x - in.uv.y) * 2.6) * plate.more.x;
        rim_hi = vec4<f32>(max(turned(rim_hi.rgb, a), vec3<f32>(0.0)), rim_hi.a);
        rim_lo = vec4<f32>(max(turned(rim_lo.rgb, a), vec3<f32>(0.0)), rim_lo.a);
        glow_c = max(turned(glow_c, a), vec3<f32>(0.0));
    }
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
    // The stone: veins through it, and the light that falls in from the top's middle.
    if plate.dress.w > 0.0 {
        slab = vec4<f32>(slab.rgb * (1.0 + plate.dress.w * veins(p)), slab.a);
    }
    if plate.dress.z > 0.0 {
        let across = 1.0 - pow(abs(in.uv.x * 2.0 - 1.0), 2.0);
        let fall = exp(-(in.uv.y * in.size.y) / 70.0);
        slab = vec4<f32>(slab.rgb + rim_hi.rgb * plate.dress.z * across * fall, slab.a);
    }
    // The band across the head: the metal's colour, thin, and a line under it.
    if plate.more.y > 0.0 {
        let y = in.uv.y * in.size.y - w;
        let on = 1.0 - smoothstep(plate.more.y - 0.5, plate.more.y + 0.5, y);
        let line = 1.0 - smoothstep(0.3, 1.0, abs(y - plate.more.y));
        let k = on * (0.10 + 0.08 * (1.0 - y / plate.more.y)) + line * 0.55;
        slab = vec4<f32>(mix(slab.rgb, rim_hi.rgb, min(k, 1.0) * step(w, inside)), slab.a);
    }
    // The inlay: a hairline set in from the rim, faint along the sides and bright in a bracket
    // at each corner.
    if plate.dress.x > 0.0 {
        let on = 1.0 - smoothstep(0.35, 1.1, abs(deep - plate.dress.x));
        let to_corner = h - abs(p);
        let reach = max(plate.orn.w, 1.0);
        let bracket = 1.0 - smoothstep(reach * 0.75, reach, max(to_corner.x, to_corner.y));
        let lit = 0.75 + 0.25 * facing;
        let k = on * plate.dress.y * (0.30 + 0.70 * bracket) * lit;
        slab = vec4<f32>(mix(slab.rgb, rim_hi.rgb, min(k, 1.0)), max(slab.a, min(k, 1.0)));
    }
    // A line of light under the top's rim: the slab's own edge.
    let lip = (1.0 - smoothstep(0.5, 2.0, deep)) * max(-n.y, 0.0) * plate.live.w;
    slab = vec4<f32>(slab.rgb + rim_hi.rgb * lip, max(slab.a, lip * rim_hi.a));
    // The glow inside the rim.
    let breath = 1.0 + plate.live.z * (0.5 * sin(globals.time * 3.2) - 0.5);
    let g = plate.glow.a * breath * exp(-deep / max(plate.shape.w, 1.0));
    slab = vec4<f32>(slab.rgb + glow_c * g, max(slab.a, min(g, 1.0)));

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
        var rim = mix(rim_lo, rim_hi, clamp(lit, 0.0, 1.0));
        // A dark hair where the rim meets the world, and where it meets the slab.
        let hair = (1.0 - smoothstep(0.0, 1.0, inside)) * 0.55
            + (1.0 - smoothstep(0.0, 1.0, abs(inside - w))) * 0.35 * step(3.0, w);
        rim = vec4<f32>(rim.rgb * (1.0 - hair), rim.a);
        let on_rim = 1.0 - smoothstep(w - 0.5, w + 0.5, inside);
        color = mix(slab, rim, on_rim);
    }

    // The light that runs round the rim: two, opposite, each trailing a tail.
    if plate.orn.y > 0.0 {
        let a = atan2(p.y / h.y, p.x / h.x) / 6.2831853;
        let at = globals.time / max(plate.orn.z, 0.1);
        let behind = fract((at - a) * 2.0);
        let spot = exp(-behind * 7.0) + 0.6 * exp(-pow((1.0 - behind) / 0.03, 2.0));
        let near = exp(-max(inside - w, 0.0) / 5.0);
        let k = spot * near * plate.orn.y;
        color = vec4<f32>(color.rgb + rim_hi.rgb * k, max(color.a, min(k, 1.0)));
    }

    // The gem in the top rim's middle: a diamond the edge cuts in half, a spark in it.
    if plate.orn.x > 0.0 {
        let s = plate.orn.x;
        let q = vec2<f32>(p.x, p.y + h.y - max(w, 1.0) * 0.5);
        let m = abs(q.x) + abs(q.y);
        if m < s + 1.5 {
            let body = 1.0 - smoothstep(s - 0.5, s + 0.5, m);
            let face = select(rim_lo.rgb, rim_hi.rgb, q.y < 0.0);
            let side = mix(0.72, 1.0, step(q.x, 0.0));
            var gem = mix(face * side, glow_c + rim_hi.rgb * 0.5, 1.0 - smoothstep(0.0, s * 0.55, m));
            gem = gem * (1.0 - 0.6 * smoothstep(s - 1.6, s - 0.4, m));
            let dark = (1.0 - smoothstep(s + 0.5, s + 1.5, m)) * 0.7;
            color = vec4<f32>(mix(color.rgb * (1.0 - dark), gem, body), max(color.a, max(body, dark)));
        }
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
        color = vec4<f32>(color.rgb + rim_hi.rgb * band * plate.live.x, max(color.a, band * plate.live.x * 0.6));
    }
    return vec4<f32>(color.rgb, color.a * cover);
}

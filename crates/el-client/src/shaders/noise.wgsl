// Noise for the materials that weather what they draw (`ground.wgsl`'s rock, `fogged.wgsl`'s
// models), after Gates' `ground_splat.wgsl` and `weathering.wgsl`: an integer hash, so it doesn't
// band at range as a `fract(sin)` one does, value noise over it, and the band a pixel can carry.
#define_import_path el::noise

// Chris Wellons' lowbias32: a full-avalanche integer hash (a `fract(sin)` hash bands at range).
fn lowbias32(x0: u32) -> u32 {
    var x = x0;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    x = x ^ (x >> 16u);
    return x;
}

fn cell_key(c: vec3<i32>) -> u32 {
    return lowbias32((bitcast<u32>(c.x) * 73856093u) ^ (bitcast<u32>(c.y) * 19349663u) ^ (bitcast<u32>(c.z) * 83492791u));
}

fn unit_of(h: u32) -> f32 {
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

fn draw3(h: u32) -> vec3<f32> {
    let a = lowbias32(h ^ 0x9e3779b9u);
    let b = lowbias32(a ^ 0x85ebca6bu);
    let c = lowbias32(b ^ 0xc2b2ae35u);
    return vec3(unit_of(a), unit_of(b), unit_of(c));
}

// Value noise over a 3D lattice, quintic-faded, 0 to 1.
fn value3(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = p - floor(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x00 = mix(unit_of(cell_key(i)), unit_of(cell_key(i + vec3(1, 0, 0))), u.x);
    let x10 = mix(unit_of(cell_key(i + vec3(0, 1, 0))), unit_of(cell_key(i + vec3(1, 1, 0))), u.x);
    let x01 = mix(unit_of(cell_key(i + vec3(0, 0, 1))), unit_of(cell_key(i + vec3(1, 0, 1))), u.x);
    let x11 = mix(unit_of(cell_key(i + vec3(0, 1, 1))), unit_of(cell_key(i + vec3(1, 1, 1))), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

// How much of a noise at `freq` cycles a metre a pixel `px` metres across can carry: whole at six
// pixels a cycle, gone at two (Gates' `weathering.wgsl`). Past that a term is speckle that crawls
// as the camera moves.
fn band(px: f32, freq: f32) -> f32 {
    return clamp(1.5 - px * freq * 3.0, 0.0, 1.0);
}

// Value noise at `freq`, faded to its mean where the pixel can't carry it.
fn value3_aa(p: vec3<f32>, freq: f32, px: f32) -> f32 {
    return mix(0.5, value3(p * freq), band(px, freq));
}

// ---- Blocks of rock (after Gates' `ground_splat.wgsl`): nearest-feature cells over 3D space, so a
// face and a ledge cut the same blocks and nothing has to choose a frame on the surface.

struct RockCell {
    // The nearest block's key, the second's (the boundary between them has a key) and the third's.
    key: u32,
    key_b: u32,
    key_c: u32,
    // How far to the boundary with the second and with the third, in cells.
    edge: f32,
    edge_c: f32,
}

fn rock_cell(p: vec3<f32>) -> RockCell {
    let base = vec3<i32>(floor(p));
    let f = p - floor(p);
    var d1 = 1e9;
    var d2 = 1e9;
    var d3 = 1e9;
    var f1 = vec3(0.0);
    var f2 = vec3(0.0);
    var f3 = vec3(0.0);
    var k1 = 0u;
    var k2 = 0u;
    var k3 = 0u;
    for (var k = -1; k <= 1; k++) {
        for (var j = -1; j <= 1; j++) {
            for (var i = -1; i <= 1; i++) {
                let o = vec3(i, j, k);
                let key = cell_key(base + o);
                let pt = vec3<f32>(o) + vec3(0.15) + draw3(key) * 0.7;
                let d = dot(pt - f, pt - f);
                if d < d1 {
                    d3 = d2; f3 = f2; k3 = k2;
                    d2 = d1; f2 = f1; k2 = k1;
                    d1 = d; f1 = pt; k1 = key;
                } else if d < d2 {
                    d3 = d2; f3 = f2; k3 = k2;
                    d2 = d; f2 = pt; k2 = key;
                } else if d < d3 {
                    d3 = d; f3 = pt; k3 = key;
                }
            }
        }
    }
    var c: RockCell;
    c.key = k1;
    c.key_b = k2;
    c.key_c = k3;
    c.edge = max(-dot(f - 0.5 * (f1 + f2), normalize(f2 - f1)), 0.0);
    c.edge_c = max(-dot(f - 0.5 * (f1 + f3), normalize(f3 - f1)), 0.0);
    return c;
}

// A block's own draw, turning over to its neighbours' across the seam rather than stepping at it
// (a step reads as an outline round every block): held whole only `hold` cells in.
fn rock_draw(c: RockCell, salt: u32, hold: f32) -> vec3<f32> {
    let w2 = 1.0 - smoothstep(0.0, hold, c.edge);
    let w3 = 1.0 - smoothstep(0.0, hold, c.edge_c);
    let sum = draw3(c.key ^ salt) + draw3(c.key_b ^ salt) * w2 + draw3(c.key_c ^ salt) * w3;
    return sum / (1.0 + w2 + w3);
}

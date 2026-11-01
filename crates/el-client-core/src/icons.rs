//! Each ability's icon, painted from its look (`fx::look`), as the effect it draws is: its family's
//! palette, a mark for how it reaches (a bolt for a skillshot, an orb for what homes, a slash for
//! a strike, a blast for an area, a ring for a field, chevrons for a dash, a star for a blink, a
//! shield or a rise for what's on the caster), a cross over what heals, a gold corner on an
//! ultimate. Two champions of a family with the same kind of ability tell apart by the mark's turn
//! and the ground's grain, which come from the champion and the slot.
//!
//! Painted once a champion's dealt, on the CPU, into RGBA8 (sRGB) for the HUD (`el-client`).

use el_sim::math::hash01;

use crate::fx::look::{Control, Reach, Rgb, ability_look};

/// An icon's side, in pixels.
pub const ICON: usize = 64;

/// Champion `champ`'s ability in `slot` (Q, W, E, R, then Blink and Tag), `ICON` × `ICON` RGBA8,
/// sRGB, rows from the top.
pub fn ability_icon(champ: u8, slot: u8) -> Vec<u8> {
    let look = ability_look(champ, slot);
    let core = shown(look.palette.core);
    let edge = shown(look.palette.edge);
    let seed = u32::from(champ) * 8 + u32::from(slot);
    // The mark's turn: a little either way, so kin don't look alike.
    let turn = (hash01(seed, 1) - 0.5) * 0.5;
    let (st, ct) = turn.sin_cos();
    let grain = 3.0 + hash01(seed, 2) * 5.0;
    // Tag is a snowball thrown, not a bolt.
    let mark = if slot == el_sim::content::champions::MARK_SLOT {
        Mark::Orb
    } else {
        Mark::of(look.reach, look.friendly)
    };
    let heals = look.friendly && !matches!(mark, Mark::Shield);
    let px = 2.0 / ICON as f32;
    let mut out = vec![0u8; ICON * ICON * 4];
    for y in 0..ICON {
        for x in 0..ICON {
            // -1..1, y up.
            let u = (x as f32 + 0.5) * px - 1.0;
            let v = 1.0 - (y as f32 + 0.5) * px;
            // The ground: the family's edge colour deep in the middle, night at the corners, a
            // light from the top left, and a faint grain across.
            let r = (u * u + v * v).sqrt();
            let lit = 0.55 + 0.25 * (v - u) * 0.5;
            let stripe = 0.5 + 0.5 * ((u + v) * grain * std::f32::consts::PI).sin();
            let deep = (1.0 - r * 0.62).max(0.0) * lit * (0.9 + 0.1 * stripe);
            let mut c = mix([0.012, 0.015, 0.025], scale(edge, 0.3), deep);
            // The mark, turned.
            let (mu, mv) = (u * ct - v * st, u * st + v * ct);
            let d = mark.sdf(mu, mv);
            // Its glow round it, then the mark itself, whitening to its middle.
            let glow = (-(d.max(0.0)) * 9.0).exp() * 0.45;
            c = add(c, scale(edge, glow));
            let cover = coverage(d, px);
            let hot = mix(core, [1.0, 1.0, 1.0], (-d * 14.0).clamp(0.0, 1.0) * 0.3);
            c = mix(c, hot, cover);
            if heals {
                let plus = seg(u, v, (0.38, 0.62), (0.78, 0.62), 0.07).min(seg(
                    u,
                    v,
                    (0.58, 0.42),
                    (0.58, 0.82),
                    0.07,
                ));
                c = mix(c, [0.55, 1.0, 0.6], coverage(plus, px));
            }
            if let Some(k) = look.control {
                // A pip at the bottom right for what it holds: the stronger, the warmer.
                let tint = match k {
                    Control::Slow => [0.45, 0.7, 1.0],
                    Control::Silence => [0.8, 0.55, 1.0],
                    Control::Displace => [1.0, 0.85, 0.5],
                    Control::Root => [0.55, 1.0, 0.45],
                    Control::Stun | Control::Airborne => [1.0, 0.9, 0.35],
                };
                let pip = ((u - 0.74).powi(2) + (v + 0.74).powi(2)).sqrt() - 0.12;
                c = mix(c, [0.0, 0.0, 0.0], coverage(pip - 0.04, px) * 0.7);
                c = mix(c, tint, coverage(pip, px));
            }
            if look.ult {
                // A gold corner at the top right.
                let corner = (1.0 - u) + (1.0 - v) - 0.42;
                c = mix(c, [1.0, 0.82, 0.4], coverage(corner, px));
            }
            // Darker at the rim, as a slot's well is.
            let rim = ((u.abs().max(v.abs()) - 0.82) / 0.18).clamp(0.0, 1.0);
            c = scale(c, 1.0 - rim * 0.45);
            let k = (y * ICON + x) * 4;
            for i in 0..3 {
                out[k + i] = (srgb(c[i]) * 255.0 + 0.5) as u8;
            }
            out[k + 3] = 255;
        }
    }
    out
}

/// The mark an ability's icon carries.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    /// A bolt, its shaft as thick as the skillshot is wide.
    Bolt(f32),
    Orb,
    Slash,
    /// A blast, ringed if it lands late.
    Blast(bool),
    Field,
    Chevrons,
    Star,
    Shield,
    Rise,
}

impl Mark {
    fn of(reach: Reach, friendly: bool) -> Mark {
        match reach {
            Reach::Line { width, .. } => Mark::Bolt((0.05 + width / 1_400.0).min(0.14)),
            Reach::Homing | Reach::Auto { .. } | Reach::Turret => Mark::Orb,
            Reach::Strike => Mark::Slash,
            Reach::Area { delay, .. } => Mark::Blast(delay > 0.0),
            Reach::Field { .. } => Mark::Field,
            Reach::Dash => Mark::Chevrons,
            Reach::Blink => Mark::Star,
            Reach::OnSelf | Reach::World if friendly => Mark::Shield,
            Reach::OnSelf | Reach::World => Mark::Rise,
        }
    }

    /// How far (-1..1 units) a point is outside the mark: negative inside.
    fn sdf(self, u: f32, v: f32) -> f32 {
        match self {
            Mark::Bolt(w) => seg(u, v, (-0.62, -0.62), (0.42, 0.42), w)
                .min(seg(u, v, (0.58, 0.58), (0.12, 0.52), 0.07))
                .min(seg(u, v, (0.58, 0.58), (0.52, 0.12), 0.07)),
            Mark::Orb => {
                let orb = circle(u, v, (0.22, 0.22), 0.3);
                let trail = circle(u, v, (-0.22, -0.22), 0.13).min(circle(u, v, (-0.5, -0.5), 0.07));
                orb.min(trail)
            }
            Mark::Slash => {
                seg(u, v, (-0.6, -0.5), (0.6, 0.5), 0.08).min(seg(u, v, (-0.5, 0.6), (0.5, -0.6), 0.05))
            }
            Mark::Blast(late) => {
                let mid = circle(u, v, (0.0, 0.0), 0.26);
                let spokes = (0..8)
                    .map(|i| {
                        let a = i as f32 * std::f32::consts::FRAC_PI_4;
                        let (s, c) = a.sin_cos();
                        seg(u, v, (c * 0.38, s * 0.38), (c * 0.62, s * 0.62), 0.045)
                    })
                    .fold(f32::MAX, f32::min);
                let ring = if late { (circle(u, v, (0.0, 0.0), 0.74)).abs() - 0.03 } else { f32::MAX };
                mid.min(spokes).min(ring)
            }
            Mark::Field => {
                let ring = circle(u, v, (0.0, 0.0), 0.58).abs() - 0.07;
                let dots = (0..6)
                    .map(|i| {
                        let a = i as f32 * std::f32::consts::FRAC_PI_3 + 0.3;
                        let (s, c) = a.sin_cos();
                        circle(u, v, (c * 0.3, s * 0.3), 0.07)
                    })
                    .fold(f32::MAX, f32::min);
                ring.min(dots)
            }
            Mark::Chevrons => (0..3)
                .map(|i| {
                    let o = -0.45 + i as f32 * 0.38;
                    let tip = (o + 0.2, o + 0.2);
                    seg(u, v, (o - 0.2, o + 0.32), tip, 0.075).min(seg(u, v, (o + 0.32, o - 0.2), tip, 0.075))
                })
                .fold(f32::MAX, f32::min),
            Mark::Star => {
                // Four long points and four short, tapering.
                let r = (u * u + v * v).sqrt();
                let a = v.atan2(u);
                let long = 0.72 * (0.5 + 0.5 * (a * 4.0).cos()).powi(6);
                let short = 0.4 * (0.5 + 0.5 * (a * 4.0 + std::f32::consts::PI).cos()).powi(6);
                r - (0.14 + long.max(short))
            }
            Mark::Shield => {
                // A heater shield, hollow, a bar down its middle.
                (shield(u, v).abs() - 0.06).min(seg(u, v, (0.0, -0.42), (0.0, 0.48), 0.05))
            }
            Mark::Rise => (0..2)
                .map(|i| {
                    let o = -0.3 + i as f32 * 0.42;
                    seg(u, v, (-0.45, o - 0.25), (0.0, o + 0.2), 0.08).min(seg(
                        u,
                        v,
                        (0.45, o - 0.25),
                        (0.0, o + 0.2),
                        0.08,
                    ))
                })
                .fold(f32::MAX, f32::min),
        }
    }
}

/// Distance outside a segment `w` thick each side.
fn seg(u: f32, v: f32, a: (f32, f32), b: (f32, f32), w: f32) -> f32 {
    let (px, py) = (u - a.0, v - a.1);
    let (bx, by) = (b.0 - a.0, b.1 - a.1);
    let t = ((px * bx + py * by) / (bx * bx + by * by).max(1e-6)).clamp(0.0, 1.0);
    let (dx, dy) = (px - bx * t, py - by * t);
    (dx * dx + dy * dy).sqrt() - w
}

/// A heater shield's outline: a flat top, round shoulders, a point below.
fn shield(u: f32, v: f32) -> f32 {
    const N: usize = 13;
    let mut pts = [(0.0f32, 0.0f32); N];
    // Down the right side from the top, to the point, up the left.
    for (i, p) in pts.iter_mut().enumerate().take(6) {
        let t = i as f32 / 5.0;
        let a = std::f32::consts::FRAC_PI_2 * t;
        *p = (0.55 * a.sin(), 0.62 - 0.1 * (1.0 - a.cos()) - t * 0.45);
    }
    pts[6] = (0.0, -0.66);
    for i in 0..6 {
        let (x, y) = pts[5 - i];
        pts[7 + i] = (-x, y);
    }
    polygon(u, v, &pts)
}

/// Signed distance to a closed polygon (negative inside).
fn polygon(u: f32, v: f32, pts: &[(f32, f32)]) -> f32 {
    let mut d = f32::MAX;
    let mut inside = false;
    let n = pts.len();
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + n - 1) % n];
        d = d.min(seg(u, v, a, b, 0.0));
        if (a.1 > v) != (b.1 > v) && u < (b.0 - a.0) * (v - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    if inside { -d } else { d }
}

fn circle(u: f32, v: f32, c: (f32, f32), r: f32) -> f32 {
    ((u - c.0).powi(2) + (v - c.1).powi(2)).sqrt() - r
}

/// How much of a pixel `px` wide a shape covers, from its distance.
fn coverage(d: f32, px: f32) -> f32 {
    (0.5 - d / px).clamp(0.0, 1.0)
}

/// A look's colour (linear, glowing past 1) brought into what a slot can show, its hue kept.
fn shown(c: Rgb) -> Rgb {
    let m = c[0].max(c[1]).max(c[2]).max(1e-3);
    c.map(|x| (x / m).powf(1.4) * 0.95)
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn add(a: Rgb, b: Rgb) -> Rgb {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: Rgb, k: f32) -> Rgb {
    a.map(|x| x * k)
}

/// Linear to sRGB's curve.
fn srgb(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use el_sim::content::champions::CHAMPIONS;

    #[test]
    fn every_ability_has_an_icon_and_none_is_blank() {
        for champ in 0..CHAMPIONS.len() as u8 {
            for slot in 0..6 {
                let px = ability_icon(champ, slot);
                assert_eq!(px.len(), ICON * ICON * 4);
                let lum: Vec<f32> =
                    px.chunks(4).map(|p| f32::from(p[0]) + f32::from(p[1]) + f32::from(p[2])).collect();
                let mean = lum.iter().sum::<f32>() / lum.len() as f32;
                let var = lum.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / lum.len() as f32;
                assert!(var.sqrt() > 40.0, "champion {champ} slot {slot}: a flat icon ({})", var.sqrt());
            }
        }
    }

    #[test]
    fn a_champions_four_icons_differ() {
        for champ in 0..CHAMPIONS.len() as u8 {
            let icons: Vec<Vec<u8>> = (0..4).map(|s| ability_icon(champ, s)).collect();
            for a in 0..4 {
                for b in a + 1..4 {
                    assert_ne!(icons[a], icons[b], "champion {champ}: slots {a} and {b} alike");
                }
            }
        }
    }

    /// Writes every champion's icons to `target/icons.ppm` to look at.
    #[test]
    #[ignore]
    fn write_sheet() {
        let n = CHAMPIONS.len();
        let (w, h) = (6 * ICON, n * ICON);
        let mut img = vec![0u8; w * h * 3];
        for champ in 0..n {
            for slot in 0..6 {
                let px = ability_icon(champ as u8, slot as u8);
                for y in 0..ICON {
                    for x in 0..ICON {
                        let s = (y * ICON + x) * 4;
                        let d = ((champ * ICON + y) * w + slot * ICON + x) * 3;
                        img[d..d + 3].copy_from_slice(&px[s..s + 3]);
                    }
                }
            }
        }
        let mut out = format!("P6 {w} {h} 255\n").into_bytes();
        out.extend_from_slice(&img);
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/icons.ppm"), out).unwrap();
    }
}

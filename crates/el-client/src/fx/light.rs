//! Light from the effects (`docs/FX.md`, the kit's *light*): a few point lights kept from the start,
//! flared for a moment where a blast lands, an ultimate is cast, a heavy hit strikes or a structure
//! falls, so the stone and the units round it catch the effect's colour and the night goes back
//! to dark after. The high tier's only (the low tier lights nothing but the moon), never more than
//! these few at once: a new flare takes the one with the least light left.

use bevy::prelude::*;
use el_client_core::fx::director::What;
use el_client_core::fx::look::Rgb;
use el_client_core::fx::sound::Part;

use super::{Catch, Cues};
use crate::coords::{S, world};
use crate::look::Look;

/// Lights in the pool.
const LIGHTS: usize = 6;

struct Flare {
    entity: Entity,
    /// When it was lit and how long it lasts (the shader's clock, s), how bright at its peak.
    born: f32,
    life: f32,
    peak: f32,
    /// It's drawn (its light on, its entity shown).
    on: bool,
}

impl Flare {
    /// How far through its life it is at `now` (past 1, or before it was lit: out).
    fn age(&self, now: f32) -> f32 {
        if self.life <= 0.0 || now < self.born { 1.0 } else { (now - self.born) / self.life }
    }

    /// Its brightness at `now`: up at once, falling away fast.
    fn brightness(&self, now: f32) -> f32 {
        let t = self.age(now);
        if t >= 1.0 { 0.0 } else { self.peak * (1.0 - t) * (1.0 - t) }
    }
}

#[derive(Resource, Default)]
pub struct Flares {
    slots: Vec<Flare>,
}

pub fn setup(mut commands: Commands, look: Res<Look>) {
    let mut flares = Flares::default();
    if look.high && look.art {
        for _ in 0..LIGHTS {
            let entity = commands
                .spawn((
                    PointLight {
                        intensity: 0.0,
                        range: 8.0,
                        radius: 0.3,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id();
            flares.slots.push(Flare { entity, born: 0.0, life: 0.0, peak: 0.0, on: false });
        }
    }
    commands.insert_resource(flares);
}

/// A palette's colour as a light's: its hue at full strength (the brightness is the light's own).
fn hue(c: Rgb) -> Color {
    let m = c[0].max(c[1]).max(c[2]).max(1e-3);
    Color::linear_rgb(c[0] / m, c[1] / m, c[2] / m)
}

/// This frame's flares, and every lit one's light as it fades.
///
/// A picture held on a caught cue lights nothing after it, and lights the cue its age ago.
pub fn flare(
    time: Res<Time>,
    cues: Res<Cues>,
    catch: Res<Catch>,
    mut flares: ResMut<Flares>,
    mut lights: Query<(&mut PointLight, &mut Transform, &mut Visibility)>,
) {
    if flares.slots.is_empty() {
        return;
    }
    let now = time.elapsed_secs_wrapped();
    let (upto, age) = match catch.cue {
        Some((i, age)) => (i + 1, age),
        None if catch.held => (0, 0.0),
        None => (cues.list.len(), 0.0),
    };
    for (i, q) in cues.list.iter().enumerate().take(upto) {
        let born = if i + 1 == upto && catch.cue.is_some() { now - age } else { now };
        // Brighter as it matters more to this viewer, as the bursts are.
        let k = 0.62 + 0.08 * f32::from(q.rating.clamp(1, 10));
        // How high it's lit from, how bright (lm), how long (s), how far it reaches (m), in what.
        let (h, peak, life, range, color) = match q.what {
            What::Part(Part::Blast) => {
                let r = (q.size * S).max(0.5);
                (0.9, 60_000.0 * r.min(3.0) * k, 0.45, 3.5 + r * 2.5, q.look.palette.core)
            }
            What::Part(Part::Ult) => (1.2, 110_000.0 * k, 0.6, 9.0, q.look.palette.core),
            What::Part(Part::Impact) if q.look.ult || q.rating >= 8 => {
                (1.1, 50_000.0 * k, 0.25, 6.0, q.look.palette.core)
            }
            What::Death(kind) if kind.is_structure() => (2.5, 450_000.0, 1.1, 15.0, q.look.palette.core),
            _ => continue,
        };
        let Some(slot) =
            flares.slots.iter_mut().min_by(|a, b| a.brightness(now).total_cmp(&b.brightness(now)))
        else {
            continue;
        };
        *slot = Flare { entity: slot.entity, born, life, peak, on: slot.on };
        if let Ok((mut light, mut tf, _)) = lights.get_mut(slot.entity) {
            light.color = hue(color);
            light.range = range;
            tf.translation = world(q.at, h);
        }
    }
    for slot in &mut flares.slots {
        let lit = slot.brightness(now);
        if lit <= 0.0 && !slot.on {
            continue;
        }
        let Ok((mut light, _, mut vis)) = lights.get_mut(slot.entity) else { continue };
        slot.on = lit > 0.0;
        light.intensity = lit;
        vis.set_if_neq(if slot.on { Visibility::Inherited } else { Visibility::Hidden });
    }
}

//! Effects (`docs/FX.md`, "The effects runtime"): `el_client_core::fx`'s director says what each
//! frame should show and play; here it's drawn. Bursts (sparks, embers, shards, dust, rings, flashes)
//! are closed forms moved in the shader ([`burst`]); what lasts (missiles' heads and trails,
//! telegraphs, fields, statuses on units) is a pool of quads placed each frame ([`live`]). The
//! sounds of the same cues are `audio.rs`'s.

pub mod burst;
pub mod live;

use bevy::prelude::*;
use el_client_core::fx::director::{Cue, Director, Status, What};
use el_client_core::fx::sound::Part;

use crate::net::now_s;
use crate::session::Session;

/// This frame's cues, for the drawing and the sound.
#[derive(Resource, Default)]
pub struct Cues {
    pub list: Vec<Cue>,
    director: Option<Director>,
    /// The client the director was made for (its seat, and how much of its feed it had read).
    of: Option<(usize, u64)>,
    /// Cues all told, and those with a sound.
    total: u64,
    sounded: u64,
}

impl Cues {
    pub fn director(&mut self) -> Option<&mut Director> {
        self.director.as_mut()
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FxSet;

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/fx_burst.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/fx_live.wgsl");
        app.init_resource::<Cues>()
            .add_plugins((
                MaterialPlugin::<burst::BurstMat>::default(),
                MaterialPlugin::<live::LiveMat>::default(),
            ))
            .add_systems(Startup, (burst::setup, live::setup))
            .add_systems(
                Update,
                (direct, burst::spawn, live::draw, shake)
                    .chain()
                    .in_set(FxSet)
                    .after(crate::net::NetSet::Send)
                    .after(crate::view::ViewSet),
            );
    }
}

/// The director's cues for this frame (a new director for each match's client).
fn direct(session: Res<Session>, mut cues: ResMut<Cues>, mut status: ResMut<crate::dev_hooks::DevStatus>) {
    cues.list.clear();
    let Some(c) = session.client.as_deref() else {
        cues.director = None;
        cues.of = None;
        return;
    };
    // A client the director wasn't made for (another match, a reload): start over, from what this
    // one has now (what came before it was the last client's).
    let fresh = cues.of.is_none_or(|(seat, read)| seat != c.seat || c.feed_n < read);
    if fresh {
        let mut d = Director::new(c.seat);
        let mut skip = Vec::new();
        d.frame(c, now_s(), &mut skip);
        cues.director = Some(d);
    }
    cues.of = Some((c.seat, c.feed_n));
    let Cues { list, director, .. } = &mut *cues;
    if let Some(d) = director.as_mut()
        && !fresh
    {
        d.frame(c, now_s(), list);
    }
    cues.total += cues.list.len() as u64;
    cues.sounded += cues.list.iter().filter(|q| q.sound.is_some()).count() as u64;
    status.set("fx_cues", cues.total as f64);
    status.set("fx_sounded", cues.sounded as f64);
}

/// A little shake for the viewer's own champion when something heavy lands on it.
fn shake(session: Res<Session>, cues: Res<Cues>, mut shake: ResMut<crate::camera::Shake>) {
    let Some(c) = session.client.as_deref() else { return };
    let max_hp = c.pred.units[c.seat].stats.max_hp.max(1.0);
    for q in cues.list.iter().filter(|q| q.unit == Some(c.seat as u8)) {
        let amp = match q.what {
            What::Status(Status::Stun | Status::Airborne | Status::Shove) => 0.10,
            What::Part(Part::Impact) if q.size > max_hp * 0.08 => 0.06 + (q.size / max_hp).min(0.3) * 0.2,
            What::Death(_) => 0.14,
            _ => continue,
        };
        shake.kick(amp, now_s());
    }
}

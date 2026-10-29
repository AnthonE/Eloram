//! Effects (`docs/FX.md`, "The effects runtime"): `el_client_core::fx`'s director says what each
//! frame should show and play; here it's drawn. Bursts (sparks, embers, shards, dust, rings, flashes)
//! are closed forms moved in the shader ([`burst`]); what lasts (missiles' heads and trails,
//! telegraphs, fields, statuses on units) is a pool of quads placed each frame ([`live`]). The
//! sounds of the same cues are `audio.rs`'s.

pub mod burst;
pub mod live;

use bevy::prelude::*;
use el_client_core::fx::director::{Cue, Director, NO_SEAT, Scene, Status, What};
use el_client_core::fx::sound::Part;

use crate::net::now_s;
use crate::session::Session;

/// This frame's cues, for the drawing and the sound.
#[derive(Resource, Default)]
pub struct Cues {
    pub list: Vec<Cue>,
    director: Option<Director>,
    /// The client the director was made for (its seat, or [`NO_SEAT`] for a match watched, and how
    /// much of its feed it had read).
    of: Option<(usize, u64)>,
    /// Cues all told, and those with a sound; the own champion's casts.
    total: u64,
    sounded: u64,
    own_casts: u64,
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

/// The director's cues for this frame (a new director for each match's client, and one for a
/// match watched).
fn direct(session: Res<Session>, mut cues: ResMut<Cues>, mut status: ResMut<crate::dev_hooks::DevStatus>) {
    cues.list.clear();
    let watched = session.watch.as_deref().map(|w| w.scene());
    let (scene, seat): (&dyn Scene, usize) = match (&watched, session.client.as_deref()) {
        (Some(w), _) => (w, NO_SEAT),
        (None, Some(c)) => (c, c.seat),
        (None, None) => {
            cues.director = None;
            cues.of = None;
            return;
        }
    };
    let (_, feed_n) = scene.feed();
    // A client the director wasn't made for (another match, a reload): start over, from what this
    // one has now (what came before it was the last client's). A match watched is shown from its
    // start.
    let fresh = cues.of.is_none_or(|(s, read)| s != seat || feed_n < read);
    if fresh {
        let d = if seat == NO_SEAT {
            Director::watching()
        } else {
            let mut d = Director::new(seat);
            let mut skip = Vec::new();
            d.frame(scene, now_s(), &mut skip);
            d
        };
        cues.director = Some(d);
    }
    cues.of = Some((seat, feed_n));
    let Cues { list, director, .. } = &mut *cues;
    if let Some(d) = director.as_mut()
        && (!fresh || seat == NO_SEAT)
    {
        d.frame(scene, now_s(), list);
    }
    // Run fast, a match watched is drawn and not heard: its sounds would come many at once.
    if session.watch.as_deref().is_some_and(|w| w.speed > 2.0) {
        for q in list.iter_mut() {
            q.sound = None;
        }
    }
    cues.total += cues.list.len() as u64;
    cues.sounded += cues.list.iter().filter(|q| q.sound.is_some()).count() as u64;
    cues.own_casts += cues.list.iter().filter(|q| q.own && q.what == What::Part(Part::Cast)).count() as u64;
    status.set("fx_cues", cues.total as f64);
    status.set("fx_sounded", cues.sounded as f64);
    status.set("fx_own_casts", cues.own_casts as f64);
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

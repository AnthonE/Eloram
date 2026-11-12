//! The champions' faces: a picture a champion (`web/assets/portraits/<name>.jpg`), shown wherever
//! a champion is named with no model of it to draw live: champion select's rows, the held
//! champion's card and the bench, the scoreboard's rows, the target frame. (The player's own
//! portrait and the allies' are drawn live: `portrait.rs`.) Each sits over its badge's initials,
//! which show until its file is in, or for good where there's none.
//!
//! The files are the live portrait's own picture, taken in practice a champion at a time
//! (`scripts/e2e.sh faces`, with `{t:"face",on:true}`: `portrait::Sitting`), so a face is its
//! model's, lit as the HUD's.

use bevy::asset::LoadState;
use bevy::prelude::*;
use el_sim::content::champions::CHAMPIONS;
use el_sim::units::{SEATS, team_of_seat};

use crate::session::{Phase, Session};

/// Whose face a picture shows.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// A teammate's row in champion select, in seat order.
    SelectRow(usize),
    /// The champion held in champion select.
    Held,
    /// A tile of the bench.
    Bench(usize),
    /// A scoreboard row: the team, and the row down it.
    Score(u8, usize),
    /// The target frame's.
    Target,
}

/// The champion in the target frame, if it's one (`hud_target.rs` says).
#[derive(Resource, Default)]
pub struct TargetFace(pub Option<u8>);

/// The pictures, in `CHAMPIONS`' order.
#[derive(Resource)]
struct Faces(Vec<Handle<Image>>);

/// A face over the whole of its badge (a child of it, after its initials): nothing until its
/// champion is known and its picture is in.
pub fn face(who: Face) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(100),
            height: percent(100),
            ..default()
        },
        ImageNode::default(),
        Visibility::Hidden,
        who,
    )
}

pub struct FacesPlugin;

impl Plugin for FacesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TargetFace>()
            .add_systems(Startup, load)
            .add_systems(Update, show.after(crate::net::NetSet::Send));
    }
}

fn load(mut commands: Commands, server: Res<AssetServer>) {
    let faces = CHAMPIONS.iter().map(|c| server.load(format!("portraits/{}.jpg", c.name.to_lowercase())));
    commands.insert_resource(Faces(faces.collect()));
}

/// A grey for a face whose champion is down.
const DOWN: Color = Color::srgb(0.35, 0.35, 0.38);

fn show(
    session: Res<Session>,
    faces: Res<Faces>,
    target: Res<TargetFace>,
    server: Res<AssetServer>,
    mut q: Query<(&Face, &mut ImageNode, &mut Visibility)>,
) {
    let select = session.select.as_ref().filter(|_| session.phase == Phase::Selecting);
    let known = |c: u8| (usize::from(c) < CHAMPIONS.len()).then_some(c);
    // The match as the scoreboard reads it: the record watched, else the world as it was sent.
    let world = match (session.watch.as_deref(), session.client.as_deref()) {
        (Some(w), _) => Some(&w.referee.m),
        (None, Some(c)) => Some(&c.world),
        (None, None) => None,
    };
    for (who, mut node, mut vis) in &mut q {
        let (def, down) = match *who {
            Face::SelectRow(i) => {
                let pick = select.and_then(|s| {
                    let team = team_of_seat(s.seat);
                    let seat = (0..SEATS).filter(|&k| team_of_seat(k) == team).nth(i)?;
                    s.picks.get(seat).copied()
                });
                (pick.and_then(known), false)
            }
            Face::Held => (select.and_then(|s| s.mine()), false),
            Face::Bench(k) => (select.and_then(|s| s.bench.get(k).copied()).and_then(known), false),
            Face::Score(t, j) => {
                let seat = (0..SEATS).filter(|&s| team_of_seat(s) == t).nth(j);
                match world.zip(seat) {
                    Some((m, s)) => (known(m.champs[s].def), m.champs[s].dead_until > m.tick),
                    None => (None, false),
                }
            }
            Face::Target => (target.0.and_then(known), false),
        };
        let picture = def
            .map(|d| &faces.0[usize::from(d)])
            .filter(|h| matches!(server.get_load_state(h.id()), Some(LoadState::Loaded)));
        let Some(h) = picture else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        if node.image != *h {
            node.image = h.clone();
        }
        let tint = if down { DOWN } else { Color::WHITE };
        if node.color != tint {
            node.color = tint;
        }
        vis.set_if_neq(Visibility::Inherited);
    }
}

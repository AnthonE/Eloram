//! The mouse and keys, as a MOBA player expects them (`docs/DESIGN.md`, "Controls"): right click
//! to move, or to attack what's under the cursor; A then a click to attack-move; Q, W, E, R and the
//! summoner spells D and F cast at once at the cursor (quick cast); Ctrl with Q, W, E or R levels it;
//! S stops, H holds; 1, 2 and 3 take an augment card, Shift with one rerolls it; P opens the shop
//! where it may be used; Tab shows the scoreboard. Each becomes an order for the next command.

use bevy::prelude::*;
use el_proto::order::slot;
use el_proto::{Aim, Order, UnitRef};
use el_sim::content::Target;
use el_sim::content::champions::ability;
use el_sim::units::SEATS;

use crate::camera::MainCamera;
use crate::coords::map_point;
use crate::session::{Phase, Session};
use crate::view::Drawn;

/// What's armed by a key and waits for a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Armed {
    AttackMove,
}

#[derive(Resource, Default)]
pub struct Controls {
    /// Orders for the next command, in the order given.
    pub pending: Vec<Order>,
    pub armed: Option<Armed>,
    /// The map point under the cursor.
    pub cursor: Option<Vec2>,
    /// The unit under the cursor.
    pub hovered: Option<usize>,
    /// Where the last move was ordered, when, and whether it was to fight on the way.
    pub marker: Option<(Vec2, f64, bool)>,
    /// Where the camera looks (the server sends what's near it first).
    pub view: Vec2,
    /// WASD, if a page ever binds it (League's keys don't: 0).
    pub steer: u8,
    pub shop_open: bool,
    pub scoreboard: bool,
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Controls>()
            .add_systems(Update, read.after(crate::net::NetSet::Receive).before(crate::net::NetSet::Send));
    }
}

const CAST_KEYS: [(KeyCode, u8); 6] = [
    (KeyCode::KeyQ, slot::Q),
    (KeyCode::KeyW, slot::W),
    (KeyCode::KeyE, slot::E),
    (KeyCode::KeyR, slot::R),
    (KeyCode::KeyD, slot::D),
    (KeyCode::KeyF, slot::F),
];

/// How near the cursor (game units, past a unit's edge) a unit counts as under it.
const PICK: f32 = 60.0;
/// How far from the cursor a targeted cast looks for a target when none is under it.
const NEAR: f32 = 300.0;

#[allow(clippy::too_many_arguments)]
fn read(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    session: Res<Session>,
    drawn: Res<Drawn>,
    ui: Query<&Interaction>,
    mut c: ResMut<Controls>,
) {
    let (cam, cam_at) = *camera;
    c.cursor = window
        .cursor_position()
        .and_then(|p| cam.viewport_to_world(cam_at, p).ok())
        .and_then(|ray| ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)))
        .map(map_point);
    c.scoreboard = keys.pressed(KeyCode::Tab);
    let Some(client) = session.client.as_ref().filter(|_| session.phase == Phase::Playing) else {
        c.hovered = None;
        return;
    };
    let seat = client.seat;
    let team = client.team;
    let me = &client.pred;
    // What's under the cursor: an enemy first, then anyone.
    c.hovered = c.cursor.and_then(|at| {
        let near = |enemies: bool| {
            drawn
                .units
                .iter()
                .enumerate()
                .filter_map(|(i, d)| d.map(|d| (i, d)))
                .filter(|(i, d)| {
                    *i != seat && (d.team != team) == enemies && d.pos.distance(at) <= d.radius + PICK
                })
                .min_by(|a, b| a.1.pos.distance(at).total_cmp(&b.1.pos.distance(at)))
                .map(|(i, _)| i)
        };
        near(true).or_else(|| near(false))
    });
    let over_ui = ui.iter().any(|i| *i != Interaction::None);
    let unit_ref =
        |i: usize| UnitRef { slot: i as u8, generation: drawn.units[i].map_or(0, |d| d.generation) };
    let now = crate::net::now_s();
    if keys.just_pressed(KeyCode::Escape) {
        c.armed = None;
    }
    if keys.just_pressed(KeyCode::KeyA) {
        c.armed = Some(Armed::AttackMove);
    }
    if mouse.just_pressed(MouseButton::Right) && !over_ui {
        c.armed = None;
        if let Some(at) = c.cursor {
            match c.hovered.filter(|&i| drawn.units[i].is_some_and(|d| d.team != team)) {
                Some(i) => c.pending.push(Order::Attack { unit: unit_ref(i) }),
                None => {
                    c.pending.push(Order::Move { to: at });
                    c.marker = Some((at, now, false));
                }
            }
        }
    }
    if mouse.just_pressed(MouseButton::Left) && !over_ui && c.armed == Some(Armed::AttackMove) {
        c.armed = None;
        if let Some(at) = c.cursor {
            c.pending.push(Order::AttackMove { to: at });
            c.marker = Some((at, now, true));
        }
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    for (key, s) in CAST_KEYS {
        if !keys.just_pressed(key) {
            continue;
        }
        if ctrl {
            if s <= slot::R {
                c.pending.push(Order::LevelSkill { slot: s });
            }
            continue;
        }
        let def = ability(me.champs[seat].def, s);
        let pick = |want_enemy: bool| {
            c.hovered.filter(|&i| drawn.units[i].is_some_and(|d| (d.team != team) == want_enemy)).or_else(
                || {
                    let at = c.cursor?;
                    drawn
                        .units
                        .iter()
                        .enumerate()
                        .filter_map(|(i, d)| d.map(|d| (i, d)))
                        .filter(|(i, d)| {
                            let ally_champ = *i < SEATS && d.team == team;
                            if want_enemy { d.team != team } else { ally_champ }
                        })
                        .map(|(i, d)| (i, d.pos.distance(at)))
                        .filter(|(_, dist)| *dist <= NEAR)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(i, _)| i)
                },
            )
        };
        let aim = match def.target {
            Target::Dir | Target::Point => c.cursor.map(Aim::Point),
            Target::Enemy => pick(true).map(|i| Aim::Unit(unit_ref(i))),
            Target::Ally => Some(Aim::Unit(unit_ref(pick(false).unwrap_or(seat)))),
            Target::Caster => Some(Aim::None),
        };
        if let Some(aim) = aim {
            c.pending.push(Order::Cast { slot: s, aim });
        }
    }
    if keys.just_pressed(KeyCode::KeyS) {
        c.pending.push(Order::Stop);
    }
    if keys.just_pressed(KeyCode::KeyH) {
        c.pending.push(Order::Hold);
    }
    for (key, card) in [(KeyCode::Digit1, 0u8), (KeyCode::Digit2, 1), (KeyCode::Digit3, 2)] {
        if keys.just_pressed(key) && me.champs[seat].screen.open {
            c.pending.push(if shift { Order::Reroll { card } } else { Order::PickAugment { card } });
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        c.shop_open = !c.shop_open;
    }
}

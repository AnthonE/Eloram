//! A first match's keys: a strip under the score with the least that plays a match in the layout
//! chosen (`el_client_core::controls::essentials`) and Esc for the rest, until the player says
//! they have it, three minutes have gone or the match is over; then never again
//! (`localStorage["el.taught"]`, `config::Prefs::taught`). Not while watching, nor in practice.

use bevy::prelude::*;
use el_client_core::controls;

use crate::config::Prefs;
use crate::dev_hooks::DevStatus;
use crate::hud::{BUTTON, DIM, FRAME, GOLD, TEXT, TRIM, Tint, framed, outline, text};
use crate::session::{Phase, Session};

/// How long into a match the strip stays, if it isn't let go sooner.
const SHOWN: f32 = 180.0;

#[derive(Component)]
struct Strip;

/// One of the strip's six: its keys (`true`) or what they do.
#[derive(Component, Clone, Copy)]
struct Chip(usize, bool);

/// The strip's button.
#[derive(Component)]
struct GotIt;

pub struct FirstPlugin;

impl Plugin for FirstPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup)
            .add_systems(Update, (got_it, show).chain().after(crate::net::NetSet::Send));
    }
}

fn setup(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(54),
                width: percent(100),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Strip,
        ))
        .with_children(|s| {
            s.spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(14),
                    padding: UiRect::axes(px(14), px(7)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                framed(),
            ))
            .with_children(|p| {
                p.spawn((text("YOUR FIRST MATCH", 11.0, GOLD), outline()));
                for k in 0..6 {
                    p.spawn(Node { align_items: AlignItems::Center, column_gap: px(6), ..default() })
                        .with_children(|c| {
                            c.spawn((
                                Node {
                                    padding: UiRect::axes(px(6), px(2)),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BorderColor::all(TRIM),
                                BackgroundColor(Color::srgb(0.05, 0.06, 0.08)),
                                children![(text("", 12.0, TEXT), TextLayout::no_wrap(), Chip(k, true))],
                            ));
                            c.spawn((text("", 12.0, DIM), outline(), TextLayout::no_wrap(), Chip(k, false)));
                        });
                }
                p.spawn((
                    Button,
                    Node {
                        height: px(26),
                        padding: UiRect::horizontal(px(10)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BorderColor::all(FRAME),
                    BackgroundColor(BUTTON),
                    Tint(BUTTON),
                    GotIt,
                    children![(text("GOT IT", 12.0, TEXT), outline())],
                ));
            });
        });
}

fn got_it(q: Query<&Interaction, (Changed<Interaction>, With<GotIt>)>, mut prefs: ResMut<Prefs>) {
    if q.iter().any(|i| *i == Interaction::Pressed) && !prefs.taught {
        prefs.set_taught();
    }
}

fn show(
    session: Res<Session>,
    menu: Res<crate::menu::Menu>,
    mut prefs: ResMut<Prefs>,
    mut strip: Query<&mut Node, With<Strip>>,
    mut chips: Query<(&Chip, &mut Text)>,
    mut status: ResMut<DevStatus>,
) {
    let playing = session.client.as_deref().filter(|c| {
        session.phase == Phase::Playing && c.started() && !c.pred.practice && session.watch.is_none()
    });
    if !prefs.taught {
        // A first match played to its end, or long enough to have found the keys.
        let over = session.phase == Phase::Over && session.client.is_some();
        if over || playing.is_some_and(|c| c.world.tick >= el_sim::ticks(SHOWN)) {
            prefs.set_taught();
        }
    }
    let on = !prefs.taught && playing.is_some() && !menu.open;
    status.set("first_strip", on);
    for mut node in &mut strip {
        let want = if on { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    if !on {
        return;
    }
    let lines = controls::essentials(prefs.layout);
    for (chip, mut t) in &mut chips {
        let (keys, does) = lines[chip.0];
        let want = if chip.1 { keys } else { does };
        if **t != want {
            **t = want.into();
        }
    }
}

//! The target frame, as League's at the top left: the unit last clicked or ordered attacked (or,
//! with none, what the champion's attacking), its name and level, its health and shield in
//! numbers, its mana, and what holds it (stunned, slowed, shielded...). Only what the team is sent:
//! an enemy's stats aren't, so they aren't shown.

use bevy::prelude::*;
use el_proto::unit::flags;
use el_sim::content::champions::CHAMPIONS;
use el_sim::units::{Kind, SEATS};

use crate::hud::{DIM, TEXT, WELL, framed, outline, text};
use crate::input::Controls;
use crate::session::{Phase, Session};
use crate::view::{CHAMPION, Drawn, TEAM};

#[derive(Component)]
struct Frame;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Words {
    Badge,
    Name,
    Level,
    Hp,
    State,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Fill {
    Hp,
    Shield,
    Mana,
}

#[derive(Component)]
struct Badge;

pub struct TargetPlugin;

impl Plugin for TargetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup).add_systems(Update, show.after(crate::net::NetSet::Send));
    }
}

fn bar(commands: &mut Commands, h: f32, fills: &[(Fill, Color)], words: Option<Words>) -> Entity {
    let e = commands
        .spawn((
            Node {
                width: percent(100),
                height: px(h),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(2)),
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            BackgroundColor(Color::srgb(0.05, 0.06, 0.08)),
        ))
        .id();
    for (f, c) in fills {
        let fill = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    height: percent(100),
                    width: percent(0),
                    ..default()
                },
                BackgroundColor(*c),
                *f,
            ))
            .id();
        commands.entity(e).add_child(fill);
    }
    if let Some(w) = words {
        let t = commands.spawn((text("", 10.0, TEXT), outline(), w)).id();
        commands.entity(e).add_child(t);
    }
    e
}

fn setup(mut commands: Commands) {
    let badge = commands
        .spawn((
            Node {
                width: px(46),
                height: px(46),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BorderColor::all(TEAM[0]),
            BackgroundColor(WELL),
            Badge,
            children![(text("", 16.0, TEXT), outline(), Words::Badge)],
        ))
        .id();
    let name = commands.spawn((text("", 13.0, TEXT), outline(), Words::Name)).id();
    let level = commands.spawn((text("", 11.0, DIM), Words::Level)).id();
    let head = commands
        .spawn(Node { justify_content: JustifyContent::SpaceBetween, width: percent(100), ..default() })
        .add_children(&[name, level])
        .id();
    let hp = bar(
        &mut commands,
        14.0,
        &[(Fill::Hp, Color::srgb(0.30, 0.78, 0.32)), (Fill::Shield, Color::srgb(0.92, 0.94, 0.97))],
        Some(Words::Hp),
    );
    let mana = bar(&mut commands, 6.0, &[(Fill::Mana, Color::srgb(0.30, 0.55, 0.98))], None);
    let state = commands.spawn((text("", 10.0, Color::srgb(0.98, 0.82, 0.36)), Words::State)).id();
    let side = commands
        .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(4), width: px(170), ..default() })
        .add_children(&[head, hp, mana, state])
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(8),
                left: px(62),
                column_gap: px(10),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(8), px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                display: Display::None,
                ..default()
            },
            framed(),
            Frame,
        ))
        .add_children(&[badge, side]);
}

/// A unit's name in the frame.
fn name_of(kind: Kind, def: Option<u8>) -> String {
    match kind {
        Kind::Champion => def.map_or("Champion", |d| CHAMPIONS[usize::from(d) % CHAMPIONS.len()].name).into(),
        Kind::Melee => "Melee minion".into(),
        Kind::Caster => "Caster minion".into(),
        Kind::Siege => "Siege minion".into(),
        Kind::Super => "Super minion".into(),
        Kind::Turret => "Turret".into(),
        Kind::Inhibitor => "Inhibitor".into(),
        Kind::Nexus => "Nexus".into(),
        Kind::Fountain => "Platform laser".into(),
    }
}

/// What holds a unit, strongest first.
fn state_of(f: u32) -> String {
    let names = [
        (flags::AIRBORNE, "Airborne"),
        (flags::STUNNED, "Stunned"),
        (flags::ROOTED, "Rooted"),
        (flags::SILENCED, "Silenced"),
        (flags::SLOWED, "Slowed"),
        (flags::SHIELDED, "Shielded"),
        (flags::IMMUNE, "Unstoppable"),
        (flags::EMPOWERED, "Empowered"),
        (flags::HASTED, "Hasted"),
        (flags::REVEALED, "Revealed"),
    ];
    let on: Vec<&str> = names.iter().filter(|(b, _)| f & b != 0).map(|(_, n)| *n).collect();
    on.join(" · ")
}

#[allow(clippy::type_complexity)]
fn show(
    session: Res<Session>,
    controls: Res<Controls>,
    drawn: Res<Drawn>,
    mut frame: Query<&mut Node, (With<Frame>, Without<Fill>)>,
    mut fills: Query<(&Fill, &mut Node), Without<Frame>>,
    mut words: Query<(&Words, &mut Text)>,
    mut badge: Query<(&mut BackgroundColor, &mut BorderColor), With<Badge>>,
) {
    let now = crate::net::now_s();
    let c = session.client.as_deref().filter(|_| session.phase == Phase::Playing && session.watch.is_none());
    // What's picked, else what the champion's attacking, while it's drawn.
    let pick = c.and_then(|c| {
        controls
            .selected
            .or_else(|| {
                let t = c.pred.units[c.seat].target;
                (!t.is_none()).then_some(usize::from(t.idx))
            })
            .filter(|&i| i != c.seat && drawn.units.get(i).is_some_and(Option::is_some))
    });
    let sample = c.zip(pick).and_then(|(c, i)| c.unit(i, now).map(|s| (c, i, s.q)));
    let Ok(mut node) = frame.single_mut() else { return };
    let d = if sample.is_some() { Display::Flex } else { Display::None };
    if node.display != d {
        node.display = d;
    }
    let Some((c, i, q)) = sample else { return };
    let kind = Kind::from_u8(q.kind);
    let def = (i < SEATS).then(|| c.world.champs[i].def);
    let max = (q.max_hp as f32).max(1.0);
    let hp = q.hp as f32 / 1_023.0 * max;
    let shield = q.shield as f32;
    let total = max.max(hp + shield);
    for (f, mut n) in &mut fills {
        let (from, share) = match f {
            Fill::Hp => (0.0, hp / total),
            Fill::Shield => (hp / total, shield / total),
            Fill::Mana => (0.0, q.mana as f32 / 127.0),
        };
        let (l, w) = (percent(from * 100.0), percent(share.clamp(0.0, 1.0) * 100.0));
        if n.left != l || n.width != w {
            n.left = l;
            n.width = w;
        }
    }
    for (w, mut t) in &mut words {
        let s = match w {
            Words::Badge => match def {
                Some(d) => CHAMPIONS[usize::from(d) % CHAMPIONS.len()]
                    .name
                    .chars()
                    .take(2)
                    .collect::<String>()
                    .to_uppercase(),
                None => name_of(kind, None).chars().next().map_or(String::new(), |ch| ch.to_string()),
            },
            Words::Name => name_of(kind, def),
            Words::Level => {
                if q.level > 0 {
                    format!("Lv {}", q.level)
                } else {
                    String::new()
                }
            }
            Words::Hp => {
                if shield > 0.0 {
                    format!("{:.0} / {:.0}  (+{:.0})", hp.ceil(), max, shield)
                } else {
                    format!("{:.0} / {:.0}", hp.ceil(), max)
                }
            }
            Words::State => state_of(q.flags),
        };
        if **t != s {
            **t = s;
        }
    }
    let team = TEAM[usize::from(q.team.min(1))];
    let fill = match def {
        Some(d) => CHAMPION[usize::from(d) % CHAMPION.len()].darker(0.25),
        None => team.darker(0.45),
    };
    for (mut bg, mut border) in &mut badge {
        bg.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(team));
    }
}

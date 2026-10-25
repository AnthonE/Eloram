//! What the player reads: their abilities and cooldowns, health, mana, level, gold and items at the
//! bottom; the clock and the kills at the top; the kill feed; the minimap; the augment cards while
//! a screen is open and the shop may be used; the shop (P); the scoreboard (Tab); and the line in
//! the middle that says what's happening (the queue, a respawn, the result). All of it read from the
//! prediction, so it moves when the player does.

use bevy::prelude::*;
use el_proto::Order;
use el_sim::content::augments::{AUGMENTS, Tier};
use el_sim::content::champions::{CHAMPIONS, ability};
use el_sim::content::items::{self, ITEMS};
use el_sim::content::rules;
use el_sim::units::{SEATS, team_of_seat};
use el_sim::{MAP_H, MAP_W, TICK_HZ};

use crate::input::Controls;
use crate::session::{Phase, Session};
use crate::view::{Drawn, FloorImage, TEAM};

const KEYS: [&str; 6] = ["Q", "W", "E", "R", "D", "F"];
const PANEL: Color = Color::srgba(0.04, 0.05, 0.08, 0.86);
const TEXT: Color = Color::srgb(0.90, 0.92, 0.95);
const DIM: Color = Color::srgb(0.55, 0.58, 0.64);
const GOLD: Color = Color::srgb(0.98, 0.82, 0.36);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Label {
    Top,
    Ping,
    Center,
    Feed,
    Stats,
    Hp,
    Mana,
    Score,
    Ability(usize),
    Card(usize),
    Item(usize),
    Shop(usize),
}

/// A bar's fill.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Fill {
    Hp,
    Mana,
    Xp,
    Cooldown(usize),
}

/// A panel shown only at times.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Bottom,
    Cards,
    Shop,
    Score,
}

/// What pressing a button does.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Act {
    Pick(u8),
    Reroll(u8),
    Buy(u8),
    Sell(u8),
}

/// A dot on the minimap, for unit slot `.0`.
#[derive(Component)]
struct Dot(usize);

/// The minimap: a left click (or drag) looks there, a right click walks there.
#[derive(Component)]
struct Minimap;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup).add_systems(
            Update,
            (
                (press, minimap).after(crate::net::NetSet::Receive).before(crate::net::NetSet::Send),
                (texts, fills, panels, dots).after(crate::net::NetSet::Send),
            ),
        );
    }
}

fn text(s: &str, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color))
}

fn setup(mut commands: Commands, floor: Res<FloorImage>) {
    // The top: the clock and the kills; the ping at the right; the kill feed under it.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(text("", 20.0, TEXT), Label::Top)],
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            right: px(10),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            row_gap: px(4),
            ..default()
        },
        children![(text("", 13.0, DIM), Label::Ping), (text("", 14.0, TEXT), Label::Feed)],
    ));
    // The middle line.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: percent(30),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(text("", 30.0, TEXT), Label::Center, TextShadow::default())],
    ));
    // The bottom: stats, abilities, bars, items.
    let abilities: Vec<Entity> = (0..6)
        .map(|k| {
            commands
                .spawn((
                    Node {
                        width: px(56),
                        height: px(56),
                        border: UiRect::all(px(2)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.25, 0.28, 0.34)),
                    BackgroundColor(Color::srgb(0.10, 0.12, 0.16)),
                    children![
                        (
                            Node {
                                position_type: PositionType::Absolute,
                                bottom: px(0),
                                left: px(0),
                                width: percent(100),
                                height: percent(0),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                            Fill::Cooldown(k),
                        ),
                        (text("", 12.0, TEXT), TextLayout::justify(Justify::Center), Label::Ability(k)),
                    ],
                ))
                .id()
        })
        .collect();
    let items_row: Vec<Entity> = (0..items::SLOTS)
        .map(|k| {
            commands
                .spawn((
                    Button,
                    Node {
                        width: px(58),
                        height: px(26),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.12, 0.13, 0.17)),
                    Act::Sell(k as u8),
                    children![(text("", 10.0, TEXT), Label::Item(k))],
                ))
                .id()
        })
        .collect();
    let bar = |commands: &mut Commands, w: f32, h: f32, color: Color, fill: Fill, label: Option<Label>| {
        let e = commands
            .spawn((
                Node {
                    width: px(w),
                    height: px(h),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.08, 0.09, 0.11)),
            ))
            .id();
        let f = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    height: percent(100),
                    width: percent(100),
                    ..default()
                },
                BackgroundColor(color),
                fill,
            ))
            .id();
        commands.entity(e).add_child(f);
        if let Some(l) = label {
            let t = commands.spawn((text("", 11.0, TEXT), l)).id();
            commands.entity(e).add_child(t);
        }
        e
    };
    let hp = bar(&mut commands, 360.0, 16.0, Color::srgb(0.25, 0.75, 0.30), Fill::Hp, Some(Label::Hp));
    let mana = bar(&mut commands, 360.0, 10.0, Color::srgb(0.25, 0.48, 0.95), Fill::Mana, Some(Label::Mana));
    let xp = bar(&mut commands, 360.0, 4.0, Color::srgb(0.72, 0.52, 0.95), Fill::Xp, None);
    let stats = commands.spawn((text("", 14.0, TEXT), Label::Stats)).id();
    let ability_row = commands.spawn(Node { column_gap: px(6), ..default() }).add_children(&abilities).id();
    let item_row = commands.spawn(Node { column_gap: px(4), ..default() }).add_children(&items_row).id();
    let panel = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(5),
                padding: UiRect::all(px(8)),
                ..default()
            },
            BackgroundColor(PANEL),
        ))
        .add_children(&[stats, ability_row, hp, mana, xp, item_row])
        .id();
    commands
        .spawn((
            // Centred in what the minimap leaves.
            Node {
                position_type: PositionType::Absolute,
                bottom: px(6),
                left: px(0),
                right: px(264),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Panel::Bottom,
        ))
        .add_child(panel);
    // The minimap, bottom right.
    let mini = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(8),
                right: px(8),
                width: px(248),
                height: px(62),
                ..default()
            },
            ImageNode::new(floor.0.clone()),
            Button,
            bevy::ui::RelativeCursorPosition::default(),
            Minimap,
        ))
        .id();
    for slot in 0..el_sim::units::FIRST_MINION {
        let size = if slot < SEATS { 7.0 } else { 5.0 };
        let d = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: px(size),
                    height: px(size),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                Visibility::Hidden,
                Dot(slot),
            ))
            .id();
        commands.entity(mini).add_child(d);
    }
    // The augment cards.
    let cards: Vec<Entity> = (0..3)
        .map(|k| {
            let reroll = commands
                .spawn((
                    Button,
                    Node { padding: UiRect::axes(px(8), px(3)), ..default() },
                    BackgroundColor(Color::srgb(0.18, 0.18, 0.22)),
                    Act::Reroll(k as u8),
                    children![text("reroll", 11.0, DIM)],
                ))
                .id();
            let card = commands
                .spawn((
                    Button,
                    Node {
                        width: px(190),
                        height: px(100),
                        border: UiRect::all(px(2)),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BorderColor::all(GOLD),
                    BackgroundColor(Color::srgb(0.10, 0.10, 0.14)),
                    Act::Pick(k as u8),
                    children![(text("", 15.0, TEXT), TextLayout::justify(Justify::Center), Label::Card(k))],
                ))
                .id();
            commands
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(4),
                    ..default()
                })
                .add_children(&[card, reroll])
                .id()
        })
        .collect();
    let row = commands.spawn(Node { column_gap: px(14), ..default() }).add_children(&cards).id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: percent(38),
                width: percent(100),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Panel::Cards,
        ))
        .add_child(row);
    // The shop.
    let shop_items: Vec<Entity> = (0..ITEMS.len())
        .map(|k| {
            commands
                .spawn((
                    Button,
                    Node {
                        width: px(150),
                        height: px(30),
                        padding: UiRect::horizontal(px(6)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.12, 0.13, 0.17)),
                    Act::Buy(k as u8),
                    children![(text("", 12.0, TEXT), Label::Shop(k))],
                ))
                .id()
        })
        .collect();
    let title = commands.spawn(text("SHOP   P closes, click an item you hold to sell it", 12.0, DIM)).id();
    let grid = commands
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            width: px(320),
            column_gap: px(6),
            row_gap: px(6),
            ..default()
        })
        .add_children(&shop_items)
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(60),
                left: px(10),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: UiRect::all(px(10)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(PANEL),
            Panel::Shop,
        ))
        .add_children(&[title, grid]);
    // The scoreboard.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: percent(20),
                width: percent(100),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Panel::Score,
        ))
        .with_child((
            Node { padding: UiRect::all(px(14)), ..default() },
            BackgroundColor(PANEL),
            children![(text("", 15.0, TEXT), Label::Score)],
        ));
}

fn mmss(ticks: u32) -> String {
    let s = ticks / TICK_HZ;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The HUD's words while watching a record.
fn watching(label: Label, w: &crate::watch::Watch) -> String {
    let m = &w.referee.m;
    let kills = |team: u8| {
        (0..SEATS).filter(|&s| team_of_seat(s) == team).map(|s| u32::from(m.champs[s].kills)).sum::<u32>()
    };
    match label {
        Label::Top => format!("{}     Dawn {}  -  {} Dusk", mmss(m.tick), kills(0), kills(1)),
        Label::Ping => {
            if w.live {
                "watching, a minute behind".into()
            } else {
                format!("replay at {}x   [ and ] change it", w.speed)
            }
        }
        Label::Center => match (&w.error, w.referee.outcome) {
            (Some(e), _) => e.clone(),
            (None, Some((tick, winner))) if m.tick >= tick => {
                format!("{} won", if winner == 0 { "Dawn" } else { "Dusk" })
            }
            _ if m.tick == 0 => "Loading the record...".into(),
            _ => String::new(),
        },
        Label::Score => {
            let mut out = String::new();
            for team in 0..2u8 {
                out.push_str(if team == 0 { "DAWN\n" } else { "\nDUSK\n" });
                for s in (0..SEATS).filter(|&s| team_of_seat(s) == team) {
                    let ch = &m.champs[s];
                    out.push_str(&format!(
                        "{:<10} {:>2}   {}/{}/{}\n",
                        CHAMPIONS[usize::from(ch.def) % CHAMPIONS.len()].name,
                        ch.level,
                        ch.kills,
                        ch.deaths,
                        ch.assists
                    ));
                }
            }
            out
        }
        _ => String::new(),
    }
}

fn texts(session: Res<Session>, controls: Res<Controls>, mut q: Query<(&Label, &mut Text)>) {
    let now = crate::net::now_s();
    let c = session.client.as_deref();
    for (label, mut t) in &mut q {
        if let Some(w) = session.watch.as_deref() {
            let s = watching(*label, w);
            if **t != s {
                **t = s;
            }
            continue;
        }
        let s: String = match (*label, c) {
            (Label::Center, _) => match session.phase {
                Phase::Connecting => "Connecting...".into(),
                Phase::SigningIn => "Sign in with your wallet...".into(),
                Phase::Watching => String::new(),
                Phase::Lobby => "In the lobby".into(),
                Phase::Queued => format!(
                    "Finding a match: {} waiting, bots take the empty seats in {} s",
                    session.waiting.max(1),
                    session.fill_in
                ),
                Phase::Failed => session.error.clone().unwrap_or_else(|| "Disconnected".into()),
                Phase::Over => match (session.winner, c) {
                    (Some(w), Some(c)) if w == c.team => "Victory".into(),
                    (Some(_), Some(_)) => "Defeat".into(),
                    _ => "The match is over".into(),
                },
                Phase::Playing => match c {
                    Some(c) if !c.started() => "Taking the field...".into(),
                    Some(c) if !c.pred.units[c.seat].alive => {
                        let left = c.pred.champs[c.seat].dead_until.saturating_sub(c.pred.tick);
                        format!("Back in {} s", left.div_ceil(TICK_HZ))
                    }
                    _ => String::new(),
                },
            },
            (Label::Top, Some(c)) => {
                let kills = |team: u8| {
                    (0..SEATS)
                        .filter(|&s| team_of_seat(s) == team)
                        .map(|s| u32::from(session.kd[s].0))
                        .sum::<u32>()
                };
                format!("{}     Dawn {}  -  {} Dusk", mmss(c.world.tick), kills(0), kills(1))
            }
            (Label::Ping, Some(c)) => {
                format!("{:.0} ms   seat {}   {}", c.clock.rtt * 1_000.0, c.seat, session.name(c.seat))
            }
            (Label::Feed, _) => session
                .feed
                .iter()
                .filter(|l| now - l.at < 12.0)
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            (Label::Stats, Some(c)) => {
                let ch = &c.pred.champs[c.seat];
                let (k, d) = session.kd[c.seat];
                let points = if ch.points > 0 {
                    format!("   {} to spend (Ctrl+key)", ch.points)
                } else {
                    String::new()
                };
                format!(
                    "{}   level {}   {:.0} gold   {k}/{d}{points}",
                    ch.def().name,
                    ch.level,
                    ch.gold.floor()
                )
            }
            (Label::Hp, Some(c)) => {
                let u = &c.pred.units[c.seat];
                format!("{:.0} / {:.0}", u.hp.max(0.0), u.stats.max_hp)
            }
            (Label::Mana, Some(c)) => {
                let u = &c.pred.units[c.seat];
                format!("{:.0} / {:.0}", u.mana.max(0.0), u.stats.max_mana)
            }
            (Label::Ability(k), Some(c)) => {
                let ch = &c.pred.champs[c.seat];
                let a = ability(ch.def, k as u8);
                let rank = if k < 4 { usize::from(ch.ranks[k]) } else { 1 };
                let left = ch.ready[k].saturating_sub(c.pred.tick);
                let cd = if rank > 0 && left > 0 {
                    format!("\n{:.0}", (left as f32 / TICK_HZ as f32).ceil())
                } else {
                    String::new()
                };
                let pips = if k < 4 { format!(" {}", "*".repeat(rank)) } else { String::new() };
                let short: String = a.name.split_whitespace().last().unwrap_or("").chars().take(7).collect();
                format!("{}{pips}\n{short}{cd}", KEYS[k])
            }
            (Label::Card(k), Some(c)) => {
                let s = &c.pred.champs[c.seat].screen;
                let a = &AUGMENTS[usize::from(s.offers[k]) % AUGMENTS.len()];
                let tier = match a.tier {
                    Tier::Silver => "Silver",
                    Tier::Gold => "Gold",
                    Tier::Prismatic => "Prismatic",
                };
                format!("{}\n{}\n[{}]", a.name, tier, k + 1)
            }
            (Label::Item(k), Some(c)) => {
                let it = c.pred.champs[c.seat].items[k];
                if it == items::NONE { String::new() } else { ITEMS[usize::from(it)].name.into() }
            }
            (Label::Shop(k), Some(c)) => {
                let gold = c.pred.champs[c.seat].gold;
                let it = &ITEMS[k];
                let mark = if gold >= it.cost as f32 { "" } else { " (short)" };
                format!("{}  {}{mark}", it.name, it.cost)
            }
            (Label::Score, Some(c)) => {
                let mut out = String::new();
                for team in 0..2u8 {
                    out.push_str(if team == 0 { "DAWN\n" } else { "\nDUSK\n" });
                    for s in (0..SEATS).filter(|&s| team_of_seat(s) == team) {
                        let def = &CHAMPIONS[usize::from(c.world.champs[s].def) % CHAMPIONS.len()];
                        let (k, d) = session.kd[s];
                        let you = if s == c.seat { "  (you)" } else { "" };
                        let level =
                            if s == c.seat { c.pred.champs[s].level } else { c.world.champs[s].level };
                        out.push_str(&format!(
                            "{:<14} {:<8} {:>2}   {k}/{d}{you}\n",
                            session.name(s),
                            def.name,
                            level
                        ));
                    }
                }
                out
            }
            _ => String::new(),
        };
        if **t != s {
            **t = s;
        }
    }
    let _ = controls;
}

fn fills(session: Res<Session>, mut q: Query<(&Fill, &mut Node)>) {
    let Some(c) = session.client.as_deref() else { return };
    let u = &c.pred.units[c.seat];
    let ch = &c.pred.champs[c.seat];
    for (f, mut node) in &mut q {
        let share = match *f {
            Fill::Hp => u.hp / u.stats.max_hp.max(1.0),
            Fill::Mana => u.mana / u.stats.max_mana.max(1.0),
            Fill::Xp => ch.xp / rules::xp_to_next(ch.level).max(1.0),
            Fill::Cooldown(k) => {
                let rank = if k < 4 { ch.ranks[k] } else { 1 };
                let left = ch.ready[k].saturating_sub(c.pred.tick) as f32;
                if rank == 0 {
                    1.0
                } else {
                    let a = ability(ch.def, k as u8);
                    let full = a.cooldown[usize::from(rank.max(1) - 1).min(4)] * TICK_HZ as f32;
                    (left / full.max(1.0)).clamp(0.0, 1.0)
                }
            }
        };
        let pct = percent((share.clamp(0.0, 1.0) * 100.0) as i32);
        match *f {
            Fill::Cooldown(_) => node.height = pct,
            _ => node.width = pct,
        }
    }
}

fn panels(session: Res<Session>, controls: Res<Controls>, mut q: Query<(&Panel, &mut Node)>) {
    let c = session.client.as_deref().filter(|c| c.started() && session.phase == Phase::Playing);
    let watching = session.watch.is_some();
    for (p, mut node) in &mut q {
        let show = match (*p, c) {
            (Panel::Score, _) if watching => controls.scoreboard,
            (Panel::Bottom, Some(_)) => true,
            (Panel::Cards, Some(c)) => c.pred.champs[c.seat].screen.open && c.pred.in_shop(c.seat),
            (Panel::Shop, Some(c)) => controls.shop_open && c.pred.in_shop(c.seat),
            (Panel::Score, Some(_)) => controls.scoreboard,
            _ => false,
        };
        let want = if show { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

fn dots(
    session: Res<Session>,
    drawn: Res<Drawn>,
    mut q: Query<(&Dot, &mut Node, &mut BackgroundColor, &mut Visibility)>,
) {
    let me = session.client.as_deref().map(|c| c.seat);
    for (d, mut node, mut bg, mut vis) in &mut q {
        match drawn.units[d.0] {
            Some(u) => {
                *vis = Visibility::Inherited;
                node.left = percent((u.pos.x / MAP_W * 100.0) as i32);
                node.top = percent(((1.0 - u.pos.y / MAP_H) * 100.0) as i32);
                bg.0 = if Some(d.0) == me { GOLD } else { TEAM[usize::from(u.team.min(1))] };
            }
            None => *vis = Visibility::Hidden,
        }
    }
}

fn minimap(
    mouse: Res<ButtonInput<MouseButton>>,
    q: Query<&bevy::ui::RelativeCursorPosition, With<Minimap>>,
    mut rig: ResMut<crate::camera::Rig>,
    mut controls: ResMut<Controls>,
    session: Res<Session>,
) {
    let Some(n) = q.iter().find(|r| r.cursor_over()).and_then(|r| r.normalized) else { return };
    // (-0.5, -0.5) is the top left: the map's far end of its first half.
    let at = Vec2::new((n.x + 0.5) * MAP_W, (0.5 - n.y) * MAP_H);
    if mouse.pressed(MouseButton::Left) {
        rig.locked = false;
        rig.focus = at;
    }
    if mouse.just_pressed(MouseButton::Right) && session.phase == Phase::Playing {
        controls.pending.push(Order::Move { to: at });
        controls.marker = Some((at, crate::net::now_s(), false));
    }
}

/// A button pressed: its order.
fn press(q: Query<(&Interaction, &Act), Changed<Interaction>>, mut controls: ResMut<Controls>) {
    for (i, act) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        controls.pending.push(match *act {
            Act::Pick(card) => Order::PickAugment { card },
            Act::Reroll(card) => Order::Reroll { card },
            Act::Buy(item) => Order::Buy { item },
            Act::Sell(inv) => Order::Sell { inv },
        });
    }
}

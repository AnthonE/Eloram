//! What the player reads, laid out as MOBA players' eyes expect it: champion select before a match
//! (their champion, their team's, the bench, a reroll, ready); then at the bottom their portrait
//! and level, the abilities (the key on each, the cooldown counted down over it, the ranks under it,
//! a "+" over it while a point may go there, its border blue when the mana's short), health and
//! mana, the items and the gold; the score and the clock at the top; the allies down the left (F2
//! to F5 look at them); the kill feed at the right; the minimap at the bottom right with the
//! camera's view on it; a tooltip over anything with something to say (an ability, an item, a card);
//! the augment cards while a screen is open and the shop may be used; the shop (P); the scoreboard
//! (Tab), with a button to open a vote to surrender from 8:00; the vote while it's open; and the
//! line in the middle that says what's happening (the queue, a respawn, the result). All of it read
//! from the prediction, so it moves when the player does.

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, BoxShadow, LinearGradient, RelativeCursorPosition, UiGlobalTransform};
use el_client_core::lobby::ToServer;
use el_client_core::tips;
use el_proto::Order;
use el_sim::content::augments::{AUGMENTS, Tier};
use el_sim::content::champions::{CHAMPIONS, ability};
use el_sim::content::items::{self, ITEMS};
use el_sim::content::rules;
use el_sim::units::{SEATS, team_of_seat};
use el_sim::{MAP_H, MAP_W, TICK_HZ};

use crate::camera::MainCamera;
use crate::coords::map_point;
use crate::input::Controls;
use crate::session::{Phase, Session};
use crate::view::{CHAMPION, Drawn, FloorImage, TEAM};

const PANEL: Color = Color::srgba(0.035, 0.045, 0.07, 0.90);
const PANEL_TOP: Color = Color::srgba(0.07, 0.085, 0.12, 0.94);
/// The frames' bronze, and a slot's well.
const FRAME: Color = Color::srgb(0.47, 0.39, 0.24);
const WELL: Color = Color::srgb(0.07, 0.08, 0.11);
const TEXT: Color = Color::srgb(0.92, 0.93, 0.96);
const DIM: Color = Color::srgb(0.56, 0.59, 0.66);
const GOLD: Color = Color::srgb(0.98, 0.82, 0.36);
const MANA: Color = Color::srgb(0.30, 0.55, 0.98);
const SHORT: Color = Color::srgb(0.90, 0.40, 0.36);
const HP: [Color; 2] = [Color::srgb(0.20, 0.62, 0.24), Color::srgb(0.38, 0.86, 0.36)];
/// Seconds the headliner stays in the middle of the screen as a match starts.
const HEADLINER_SHOWN: u32 = 12;
/// Allies down the left.
const ALLIES: usize = el_sim::units::TEAM_SIZE - 1;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Label {
    KillsDawn,
    KillsDusk,
    Clock,
    Ping,
    Center,
    Feed,
    Name,
    Kda,
    Level,
    Initials,
    Gold,
    Hp,
    Mana,
    Key(usize),
    AbilityName(usize),
    Cooldown(usize),
    CardTier(usize),
    CardName(usize),
    CardText(usize),
    CardKey(usize),
    CardReroll,
    Item(usize),
    Shop(usize),
    ShopCost(usize),
    ShopGold,
    ScoreHead,
    ScoreTeam(u8),
    SelectTitle,
    SelectMine,
    SelectTeam,
    SelectReroll,
    SelectReady,
    Bench(usize),
    Vote,
    VoteYes,
    VoteNo,
    Surrender,
    AllyName(usize),
    AllyLevel(usize),
    AllyInitials(usize),
    TipTitle,
    TipBody,
}

/// A bar's fill.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Fill {
    Hp,
    Mana,
    Xp,
    Cooldown(usize),
    AllyHp(usize),
}

/// A panel shown only at times.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Top,
    Bottom,
    Allies,
    Feed,
    Cards,
    Shop,
    Score,
    Select,
    /// The team's vote to surrender, while it's open.
    Vote,
    /// The button that opens one, with the scoreboard, from the time one may.
    Surrender,
    Tip,
}

/// What pressing a button does.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Act {
    Pick(u8),
    Reroll(u8),
    Buy(u8),
    Sell(u8),
    /// A point into Q, W, E or R (the "+" over it).
    Level(u8),
    /// Champion select: take the bench's `.0`th champion.
    Swap(usize),
    RerollChampion,
    Ready,
    /// A vote to surrender: yes (which opens one when none is) or no.
    Vote(bool),
}

/// What a tooltip over this says.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Tip {
    Ability(usize),
    Item(usize),
    Shop(usize),
}

/// A button's colour at rest: lighter hovered, darker pressed.
#[derive(Component, Clone, Copy)]
struct Tint(Color);

/// An ability's slot (its border says ready, cooling down, short of mana or not learned).
#[derive(Component)]
struct AbilitySlot(usize);

/// One of an ability's rank marks.
#[derive(Component)]
struct Pip(usize, u8);

/// The "+" over an ability a point may go to.
#[derive(Component)]
struct LevelUp(usize);

/// An augment card's frame, in its tier's colour.
#[derive(Component)]
struct CardFrame(usize);

/// The player's portrait (`None`) or an ally's, in the champion's colour.
#[derive(Component)]
struct Portrait(Option<usize>);

/// An ally's row, shown while there's an ally for it.
#[derive(Component)]
struct AllyRow(usize);

/// A place on the bench in champion select, shown while the bench reaches it.
#[derive(Component)]
struct BenchSlot(usize);

/// Places on a team's bench (it holds at most what was never dealt).
const BENCH: usize = 10;

/// A dot on the minimap, for unit slot `.0`.
#[derive(Component)]
struct Dot(usize);

/// The minimap: a left click (or drag) looks there, a right click walks there.
#[derive(Component)]
struct Minimap;

/// The camera's view on the minimap.
#[derive(Component)]
struct ViewBox;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup).add_systems(
            Update,
            (
                (press, minimap).after(crate::net::NetSet::Receive).before(crate::net::NetSet::Send),
                (texts, fills, panels, dots, bench_slots, view_box, tooltip).after(crate::net::NetSet::Send),
                (scale, tints, slots, pips, level_ups, card_frames, portraits, ally_rows, colors)
                    .after(crate::net::NetSet::Send),
            ),
        );
    }
}

fn text(s: &str, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color))
}

/// A tight shadow under words, to read them over the bright ground.
fn outline() -> TextShadow {
    TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) }
}

/// A soft shadow under a panel.
fn shadow() -> BoxShadow {
    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.55), px(0), px(4), px(0), px(14))
}

/// A vertical gradient, `top` to `bottom`.
fn down(top: Color, bottom: Color) -> BackgroundGradient {
    BackgroundGradient(vec![LinearGradient::to_bottom(vec![top.into(), bottom.into()]).into()])
}

/// A framed panel's look. It takes the cursor: a click on it is the HUD's, not the map's.
fn framed() -> (BorderColor, BackgroundGradient, BoxShadow, Interaction) {
    (BorderColor::all(FRAME), down(PANEL_TOP, PANEL), shadow(), Interaction::None)
}

/// A colour a little lighter (`by` > 0) or darker.
fn shade(c: Color, by: f32) -> Color {
    let s = c.to_srgba();
    Color::srgba(
        (s.red + by).clamp(0.0, 1.0),
        (s.green + by).clamp(0.0, 1.0),
        (s.blue + by).clamp(0.0, 1.0),
        s.alpha,
    )
}

/// The first two letters of a champion's name, for its portrait.
fn initials(def: u8) -> String {
    CHAMPIONS
        .get(usize::from(def))
        .map_or(String::new(), |d| d.name.chars().take(2).collect::<String>().to_uppercase())
}

/// A bar: its well, its fill and, if any, the words on it.
fn bar(
    commands: &mut Commands,
    w: Val,
    h: f32,
    fill_color: [Color; 2],
    fill: Fill,
    label: Option<(Label, f32)>,
) -> Entity {
    let e = commands
        .spawn((
            Node {
                width: w,
                height: px(h),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            BackgroundColor(Color::srgb(0.05, 0.06, 0.08)),
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
            down(fill_color[1], fill_color[0]),
            fill,
        ))
        .id();
    commands.entity(e).add_child(f);
    if let Some((l, size)) = label {
        let t = commands.spawn((text("", size, TEXT), outline(), l)).id();
        commands.entity(e).add_child(t);
    }
    e
}

/// A button with words on it.
fn button(commands: &mut Commands, act: Act, label: Label, width: f32, base: Color) -> Entity {
    commands
        .spawn((
            Button,
            Node {
                width: px(width),
                height: px(32),
                padding: UiRect::horizontal(px(8)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BorderColor::all(FRAME),
            BackgroundColor(base),
            Tint(base),
            act,
            children![(text("", 13.0, TEXT), label)],
        ))
        .id()
}

const BUTTON: Color = Color::srgb(0.11, 0.12, 0.16);
const BUTTON_GOLD: Color = Color::srgb(0.30, 0.24, 0.10);

#[allow(clippy::too_many_lines)]
fn setup(mut commands: Commands, floor: Res<FloorImage>) {
    // The top: the kills each side of the clock.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(0),
            width: percent(100),
            justify_content: JustifyContent::Center,
            display: Display::None,
            ..default()
        },
        Panel::Top,
        children![(
            Node {
                align_items: AlignItems::Center,
                column_gap: px(14),
                padding: UiRect::axes(px(18), px(5)),
                border: UiRect::new(px(1), px(1), px(0), px(1)),
                border_radius: BorderRadius::px(0.0, 0.0, 10.0, 10.0),
                ..default()
            },
            framed(),
            children![
                (text("DAWN", 11.0, TEAM[0]), Node { margin: UiRect::top(px(4)), ..default() }),
                (text("0", 22.0, TEAM[0]), outline(), Label::KillsDawn),
                (
                    Node {
                        min_width: px(64),
                        justify_content: JustifyContent::Center,
                        padding: UiRect::axes(px(8), px(2)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                    children![(text("0:00", 16.0, TEXT), Label::Clock)],
                ),
                (text("0", 22.0, TEAM[1]), outline(), Label::KillsDusk),
                (text("DUSK", 11.0, TEAM[1]), Node { margin: UiRect::top(px(4)), ..default() }),
            ],
        )],
    ));
    // The ping at the right, the kill feed under it.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            right: px(10),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            row_gap: px(6),
            ..default()
        },
        children![
            (text("", 12.0, DIM), outline(), Label::Ping),
            (
                Node {
                    padding: UiRect::axes(px(10), px(6)),
                    border_radius: BorderRadius::all(px(6)),
                    max_width: px(360),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.62)),
                Panel::Feed,
                children![(
                    text("", 13.0, TEXT),
                    outline(),
                    TextLayout::justify(Justify::Right),
                    Label::Feed
                )],
            ),
        ],
    ));
    // The middle line.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: percent(28),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            text("", 30.0, TEXT),
            TextLayout::justify(Justify::Center),
            TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) },
            Label::Center
        )],
    ));
    bottom(&mut commands);
    allies(&mut commands);
    // The minimap, bottom right, in a frame.
    let mini = commands
        .spawn((
            Node { width: px(320), height: px(80), overflow: Overflow::clip(), ..default() },
            ImageNode::new(floor.0.clone()),
            Button,
            RelativeCursorPosition::default(),
            Minimap,
        ))
        .id();
    for slot in 0..el_sim::units::FIRST_MINION {
        let champ = slot < SEATS;
        let size = if champ { 9.0 } else { 6.0 };
        let d = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: px(size),
                    height: px(size),
                    margin: UiRect::new(px(-size / 2.0), px(0), px(-size / 2.0), px(0)),
                    border: UiRect::all(px(1)),
                    border_radius: if champ {
                        BorderRadius::all(percent(50))
                    } else {
                        BorderRadius::all(px(1))
                    },
                    ..default()
                },
                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.9)),
                BackgroundColor(Color::NONE),
                Visibility::Hidden,
                ZIndex(i32::from(champ)),
                Dot(slot),
            ))
            .id();
        commands.entity(mini).add_child(d);
    }
    let view = commands
        .spawn((
            Node { position_type: PositionType::Absolute, border: UiRect::all(px(2)), ..default() },
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.8)),
            Visibility::Hidden,
            ViewBox,
        ))
        .id();
    commands.entity(mini).add_child(view);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(6),
                right: px(6),
                padding: UiRect::all(px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            framed(),
        ))
        .add_child(mini);
    cards(&mut commands);
    shop(&mut commands);
    // The scoreboard.
    let teams: Vec<Entity> = (0..2u8)
        .map(|t| {
            commands
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6),
                        padding: UiRect::all(px(10)),
                        border: UiRect::left(px(3)),
                        border_radius: BorderRadius::all(px(4)),
                        min_width: px(330),
                        ..default()
                    },
                    BorderColor::all(TEAM[usize::from(t)]),
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
                    children![
                        text(if t == 0 { "DAWN" } else { "DUSK" }, 14.0, TEAM[usize::from(t)]),
                        text(&format!("{:<14} {:<10} {:>2}   K/D", "Name", "Champion", "Lv"), 13.0, DIM),
                        (text("", 13.0, TEXT), Label::ScoreTeam(t)),
                    ],
                ))
                .id()
        })
        .collect();
    let team_row = commands.spawn(Node { column_gap: px(12), ..default() }).add_children(&teams).id();
    let head = commands.spawn((text("", 14.0, GOLD), Label::ScoreHead)).id();
    let body = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(16)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            framed(),
        ))
        .add_children(&[head, team_row])
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: percent(18),
                width: percent(100),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Panel::Score,
        ))
        .add_child(body);
    select(&mut commands);
    // The vote to surrender, and the button that opens one.
    let title = commands.spawn((text("", 15.0, GOLD), Label::Vote)).id();
    let yes = button(&mut commands, Act::Vote(true), Label::VoteYes, 90.0, BUTTON);
    let no = button(&mut commands, Act::Vote(false), Label::VoteNo, 90.0, BUTTON);
    let body = commands
        .spawn((
            Node {
                column_gap: px(10),
                align_items: AlignItems::Center,
                padding: UiRect::all(px(10)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            framed(),
        ))
        .add_children(&[title, yes, no])
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: percent(9),
                width: percent(100),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Panel::Vote,
        ))
        .add_child(body);
    let open = button(&mut commands, Act::Vote(true), Label::Surrender, 140.0, BUTTON);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: percent(12),
                width: percent(100),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Panel::Surrender,
        ))
        .add_child(open);
    // The tooltip, over everything, put over what it's about. Its width is set, so its words wrap
    // before it's measured.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            width: px(TIP_W),
            padding: UiRect::axes(px(10), px(8)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
            display: Display::None,
            ..default()
        },
        BorderColor::all(FRAME),
        down(Color::srgb(0.10, 0.11, 0.15), Color::srgb(0.05, 0.06, 0.09)),
        shadow(),
        GlobalZIndex(20),
        Panel::Tip,
        children![(text("", 14.0, GOLD), Label::TipTitle), (text("", 12.0, TEXT), Label::TipBody)],
    ));
}

/// The bottom: the portrait and level, the abilities, health and mana, the items and the gold.
fn bottom(commands: &mut Commands) {
    // The portrait, its level on a badge, the experience under it.
    let badge = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(-7),
                bottom: px(-7),
                width: px(24),
                height: px(24),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(percent(50)),
                ..default()
            },
            BorderColor::all(GOLD),
            BackgroundColor(Color::srgb(0.06, 0.07, 0.10)),
            children![(text("1", 12.0, TEXT), Label::Level)],
        ))
        .id();
    let portrait = commands
        .spawn((
            Node {
                width: px(66),
                height: px(66),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BorderColor::all(GOLD),
            BackgroundColor(WELL),
            Portrait(None),
            children![(text("", 26.0, TEXT), outline(), Label::Initials)],
        ))
        .add_child(badge)
        .id();
    let xp = bar(
        commands,
        px(66),
        5.0,
        [Color::srgb(0.50, 0.32, 0.80), Color::srgb(0.74, 0.55, 0.98)],
        Fill::Xp,
        None,
    );
    let left = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(9),
            ..default()
        })
        .add_children(&[portrait, xp])
        .id();
    // The abilities: Q, W, E and R, then the summoner spells a little apart and smaller.
    let slots: Vec<Entity> = (0..6).map(|k| ability_slot(commands, k)).collect();
    let gap = commands.spawn(Node { width: px(8), ..default() }).id();
    let row = commands
        .spawn(Node { column_gap: px(6), align_items: AlignItems::End, ..default() })
        .add_children(&slots[..4])
        .add_child(gap)
        .add_children(&slots[4..])
        .id();
    let name = commands.spawn((text("", 13.0, TEXT), Label::Name)).id();
    let kda = commands.spawn((text("", 13.0, DIM), Label::Kda)).id();
    let head = commands
        .spawn(Node { justify_content: JustifyContent::SpaceBetween, width: percent(100), ..default() })
        .add_children(&[name, kda])
        .id();
    let hp = bar(commands, percent(100), 18.0, HP, Fill::Hp, Some((Label::Hp, 12.0)));
    let mana = bar(
        commands,
        percent(100),
        12.0,
        [Color::srgb(0.16, 0.34, 0.80), MANA],
        Fill::Mana,
        Some((Label::Mana, 10.0)),
    );
    let middle = commands
        .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(5), width: px(372), ..default() })
        .add_children(&[head, row, hp, mana])
        .id();
    // The items, three by two, and the gold under them.
    let items_row: Vec<Entity> = (0..items::SLOTS)
        .map(|k| {
            commands
                .spawn((
                    Button,
                    Node {
                        width: px(42),
                        height: px(42),
                        padding: UiRect::all(px(2)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.24, 0.22, 0.18)),
                    BackgroundColor(WELL),
                    Tint(WELL),
                    Act::Sell(k as u8),
                    Tip::Item(k),
                    children![(text("", 9.0, TEXT), TextLayout::justify(Justify::Center), Label::Item(k))],
                ))
                .id()
        })
        .collect();
    let grid = commands
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            width: px(138),
            column_gap: px(4),
            row_gap: px(4),
            ..default()
        })
        .add_children(&items_row)
        .id();
    let gold = commands
        .spawn((
            Node {
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(6), px(3)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            children![(text("", 15.0, GOLD), outline(), Label::Gold)],
        ))
        .id();
    let right = commands
        .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(5), ..default() })
        .add_children(&[grid, gold])
        .id();
    let panel = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(14),
                padding: UiRect::new(px(14), px(14), px(10), px(10)),
                border: UiRect::new(px(1), px(1), px(1), px(0)),
                border_radius: BorderRadius::px(12.0, 12.0, 0.0, 0.0),
                ..default()
            },
            framed(),
        ))
        .add_children(&[left, middle, right])
        .id();
    commands
        .spawn((
            // Centred in what the minimap leaves.
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0),
                left: px(0),
                right: px(340),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Panel::Bottom,
        ))
        .add_child(panel);
}

/// An ability's slot: the "+" over it, the slot (its name, the cooldown's shade and count, its key)
/// and its ranks under it.
fn ability_slot(commands: &mut Commands, k: usize) -> Entity {
    let size = if k < 4 { 54.0 } else { 42.0 };
    let plus = commands
        .spawn((
            Button,
            Node {
                width: px(22),
                height: px(14),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BorderColor::all(GOLD),
            BackgroundColor(BUTTON_GOLD),
            Tint(BUTTON_GOLD),
            Visibility::Hidden,
            LevelUp(k),
            Act::Level(k as u8),
            children![(text("+", 12.0, GOLD), Node { margin: UiRect::top(px(-3)), ..default() })],
        ))
        .id();
    let slot = commands
        .spawn((
            Node {
                width: px(size),
                height: px(size),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(FRAME),
            down(Color::srgb(0.16, 0.18, 0.24), Color::srgb(0.07, 0.08, 0.11)),
            Interaction::None,
            AbilitySlot(k),
            Tip::Ability(k),
            children![
                (text("", 9.0, DIM), TextLayout::justify(Justify::Center), Label::AbilityName(k)),
                (
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: px(0),
                        left: px(0),
                        width: percent(100),
                        height: percent(0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.66)),
                    Fill::Cooldown(k),
                ),
                (
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    children![(
                        text("", if k < 4 { 20.0 } else { 16.0 }, TEXT),
                        outline(),
                        Label::Cooldown(k)
                    )],
                ),
                (
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        bottom: px(0),
                        padding: UiRect::axes(px(3), px(0)),
                        border_radius: BorderRadius::px(0.0, 4.0, 0.0, 0.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                    children![(text("", 10.0, GOLD), Label::Key(k))],
                ),
            ],
        ))
        .id();
    let ranks = match k {
        0..=2 => 5,
        3 => 3,
        _ => 0,
    };
    let pips: Vec<Entity> = (0..ranks)
        .map(|r| {
            commands
                .spawn((
                    Node {
                        width: px(7),
                        height: px(3),
                        border_radius: BorderRadius::all(px(1)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.20, 0.21, 0.25)),
                    Pip(k, r),
                ))
                .id()
        })
        .collect();
    let pip_row =
        commands.spawn(Node { column_gap: px(2), height: px(3), ..default() }).add_children(&pips).id();
    commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(3),
            ..default()
        })
        .add_children(&[plus, slot, pip_row])
        .id()
}

/// The allies down the left: a portrait with the level, the name, the health; F2 to F5 look.
fn allies(commands: &mut Commands) {
    let rows: Vec<Entity> = (0..ALLIES)
        .map(|i| {
            let portrait = commands
                .spawn((
                    Node {
                        width: px(38),
                        height: px(38),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BorderColor::all(FRAME),
                    BackgroundColor(WELL),
                    Portrait(Some(i)),
                    children![
                        (text("", 14.0, TEXT), outline(), Label::AllyInitials(i)),
                        (
                            Node {
                                position_type: PositionType::Absolute,
                                right: px(-5),
                                bottom: px(-5),
                                min_width: px(16),
                                height: px(16),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(percent(50)),
                                ..default()
                            },
                            BorderColor::all(FRAME),
                            BackgroundColor(Color::srgb(0.06, 0.07, 0.10)),
                            children![(text("", 9.0, TEXT), Label::AllyLevel(i))],
                        ),
                    ],
                ))
                .id();
            let name = commands.spawn((text("", 11.0, TEXT), outline(), Label::AllyName(i))).id();
            let hp = bar(commands, px(86), 6.0, HP, Fill::AllyHp(i), None);
            let side = commands
                .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(4), ..default() })
                .add_children(&[name, hp])
                .id();
            commands
                .spawn((
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(10),
                        padding: UiRect::new(px(6), px(10), px(6), px(6)),
                        border_radius: BorderRadius::px(0.0, 8.0, 8.0, 0.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.62)),
                    Interaction::None,
                    AllyRow(i),
                ))
                .add_children(&[portrait, side])
                .id()
        })
        .collect();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: percent(26),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                display: Display::None,
                ..default()
            },
            Panel::Allies,
        ))
        .add_children(&rows);
}

/// The augment cards, a frame each in its tier's colour.
fn cards(commands: &mut Commands) {
    let cards: Vec<Entity> = (0..3)
        .map(|k| {
            let reroll = button(commands, Act::Reroll(k as u8), Label::CardReroll, 100.0, BUTTON);
            let card = commands
                .spawn((
                    Button,
                    Node {
                        width: px(210),
                        height: px(150),
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(10)),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(px(10)),
                        ..default()
                    },
                    BorderColor::all(GOLD),
                    BackgroundColor(Color::srgb(0.08, 0.09, 0.13)),
                    Tint(Color::srgb(0.08, 0.09, 0.13)),
                    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), px(0), px(6), px(0), px(18)),
                    CardFrame(k),
                    Act::Pick(k as u8),
                    children![
                        (text("", 11.0, GOLD), Label::CardTier(k)),
                        (text("", 17.0, TEXT), TextLayout::justify(Justify::Center), Label::CardName(k)),
                        (text("", 11.0, DIM), TextLayout::justify(Justify::Center), Label::CardText(k)),
                        (text("", 11.0, DIM), Label::CardKey(k)),
                    ],
                ))
                .id();
            commands
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(6),
                    ..default()
                })
                .add_children(&[card, reroll])
                .id()
        })
        .collect();
    let title = commands.spawn((text("CHOOSE AN AUGMENT", 18.0, GOLD), outline())).id();
    let row = commands.spawn(Node { column_gap: px(16), ..default() }).add_children(&cards).id();
    let body = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(12),
            ..default()
        })
        .add_children(&[title, row])
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: percent(30),
                width: percent(100),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Panel::Cards,
        ))
        .add_child(body);
}

/// The shop: the items in two columns, each with its price, the gold at the top.
fn shop(commands: &mut Commands) {
    let shop_items: Vec<Entity> = (0..ITEMS.len())
        .map(|k| {
            commands
                .spawn((
                    Button,
                    Node {
                        width: px(176),
                        height: px(32),
                        padding: UiRect::horizontal(px(8)),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.22, 0.21, 0.18)),
                    BackgroundColor(BUTTON),
                    Tint(BUTTON),
                    Act::Buy(k as u8),
                    Tip::Shop(k),
                    children![
                        (text("", 12.0, TEXT), Label::Shop(k)),
                        (text("", 12.0, GOLD), Label::ShopCost(k))
                    ],
                ))
                .id()
        })
        .collect();
    let title = commands.spawn(text("SHOP", 15.0, GOLD)).id();
    let gold = commands.spawn((text("", 14.0, GOLD), Label::ShopGold)).id();
    let head = commands
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .add_children(&[title, gold])
        .id();
    let hint = commands.spawn(text("P closes. Click an item you hold to sell it.", 11.0, DIM)).id();
    let grid = commands
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            width: px(358),
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
                row_gap: px(8),
                padding: UiRect::all(px(12)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                display: Display::None,
                ..default()
            },
            framed(),
            Panel::Shop,
        ))
        .add_children(&[head, hint, grid]);
}

/// Champion select: your champion, your team's, the bench, a reroll, ready.
fn select(commands: &mut Commands) {
    let bench: Vec<Entity> = (0..BENCH)
        .map(|k| {
            let b = button(commands, Act::Swap(k), Label::Bench(k), 120.0, BUTTON);
            commands.entity(b).insert(BenchSlot(k));
            b
        })
        .collect();
    let bench_row = commands
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            max_width: px(640),
            column_gap: px(6),
            row_gap: px(6),
            ..default()
        })
        .add_children(&bench)
        .id();
    let reroll = button(commands, Act::RerollChampion, Label::SelectReroll, 170.0, BUTTON);
    let ready = button(commands, Act::Ready, Label::SelectReady, 240.0, BUTTON_GOLD);
    let actions = commands
        .spawn(Node { column_gap: px(12), margin: UiRect::top(px(6)), ..default() })
        .add_children(&[reroll, ready])
        .id();
    let title = commands.spawn((text("", 14.0, GOLD), Label::SelectTitle)).id();
    let mine = commands.spawn((text("", 28.0, TEXT), outline(), Label::SelectMine)).id();
    let team = commands.spawn((text("", 13.0, TEXT), Label::SelectTeam)).id();
    let bench_title = commands
        .spawn(text("THE BENCH: click a champion to take it, yours goes in its place", 11.0, DIM))
        .id();
    let body = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(22)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            framed(),
        ))
        .add_children(&[title, mine, team, bench_title, bench_row, actions])
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: percent(16),
                width: percent(100),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Panel::Select,
        ))
        .add_child(body);
}

/// Champion `c`'s name, or a dash if there's none to show.
fn champ_name(c: u8) -> &'static str {
    CHAMPIONS.get(usize::from(c)).map_or("-", |d| d.name)
}

/// The words of champion select's panel.
fn select_text(label: Label, session: &Session) -> String {
    let Some(s) = session.select.as_ref() else { return String::new() };
    let left = (s.ends_at - crate::net::now_s()).max(0.0).ceil() as u32;
    match label {
        Label::SelectTitle => format!("CHAMPION SELECT   {left} s"),
        Label::SelectMine => match s.mine().map(|c| &CHAMPIONS[usize::from(c)]) {
            Some(d) => format!("{}, {}\n{}", d.name, d.title, d.role.name()),
            None => String::new(),
        },
        Label::SelectTeam => {
            let team = team_of_seat(s.seat);
            let seats: Vec<String> = (0..SEATS)
                .filter(|&k| team_of_seat(k) == team)
                .map(|k| {
                    let you = if k == s.seat { " (you)" } else { "" };
                    format!(
                        "{} {}{you}",
                        session.name(k),
                        champ_name(s.picks.get(k).copied().unwrap_or(u8::MAX))
                    )
                })
                .collect();
            format!("{}: {}", if team == 0 { "DAWN" } else { "DUSK" }, seats.join(",  "))
        }
        Label::SelectReroll => {
            if s.rerolls > 0 {
                format!("Reroll ({} left)", s.rerolls)
            } else {
                "No rerolls left".into()
            }
        }
        Label::SelectReady => {
            if s.ready {
                "Ready: waiting for the others".into()
            } else {
                "Ready".into()
            }
        }
        Label::Bench(k) => s.bench.get(k).map_or(String::new(), |&c| champ_name(c).into()),
        _ => String::new(),
    }
}

/// Shows as many bench places as the bench holds.
fn bench_slots(session: Res<Session>, mut q: Query<(&BenchSlot, &mut Node)>) {
    let n = session.select.as_ref().map_or(0, |s| s.bench.len());
    for (slot, mut node) in &mut q {
        let want = if slot.0 < n { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

fn mmss(ticks: u32) -> String {
    let s = ticks / TICK_HZ;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A seat's line on the scoreboard.
struct Row {
    name: String,
    def: u8,
    level: u8,
    kills: u16,
    deaths: u16,
    you: bool,
}

/// A team's rows on the scoreboard: name, champion, level, kills and deaths.
fn score_rows(team: u8, row: impl Fn(usize) -> Row) -> String {
    (0..SEATS)
        .filter(|&s| team_of_seat(s) == team)
        .map(|s| {
            let r = row(s);
            let you = if r.you { "  <" } else { "" };
            format!(
                "{:<14} {:<10} {:>2}   {}/{}{you}",
                r.name.chars().take(14).collect::<String>(),
                champ_name(r.def % CHAMPIONS.len() as u8),
                r.level,
                r.kills,
                r.deaths
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The HUD's words while watching a record.
fn watching(label: Label, w: &crate::watch::Watch, session: &Session, now: f64) -> String {
    let m = &w.referee.m;
    let kills = |team: u8| {
        (0..SEATS).filter(|&s| team_of_seat(s) == team).map(|s| u32::from(m.champs[s].kills)).sum::<u32>()
    };
    match label {
        Label::Clock => mmss(m.tick),
        Label::KillsDawn => kills(0).to_string(),
        Label::KillsDusk => kills(1).to_string(),
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
        Label::ScoreHead => {
            let h = m.headliner();
            format!("{}: {}", h.name, h.blurb)
        }
        Label::ScoreTeam(t) => score_rows(t, |s| {
            let ch = &m.champs[s];
            Row {
                name: session.name(s).to_string(),
                def: ch.def,
                level: ch.level,
                kills: ch.kills,
                deaths: ch.deaths,
                you: false,
            }
        }),
        Label::Feed => feed_text(session, now),
        _ => String::new(),
    }
}

/// The kill feed's lines of the last twelve seconds.
fn feed_text(session: &Session, now: f64) -> String {
    session.feed.iter().filter(|l| now - l.at < 12.0).map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
}

/// The seats of the player's allies, in seat order (F2 to F5's).
fn ally_seats(c: &el_client_core::Client) -> impl Iterator<Item = usize> + '_ {
    (0..SEATS).filter(move |&s| s != c.seat && team_of_seat(s) == c.team)
}

/// Whether a point may go into ability `k` now.
fn may_level(c: &el_client_core::Client, k: usize) -> bool {
    let ch = &c.pred.champs[c.seat];
    k < 4 && ch.points > 0 && ch.ranks[k] < el_sim::Match::max_rank(k as u8, ch.level)
}

fn texts(session: Res<Session>, controls: Res<Controls>, mut q: Query<(&Label, &mut Text)>) {
    let now = crate::net::now_s();
    let c = session.client.as_deref();
    for (label, mut t) in &mut q {
        if matches!(label, Label::TipTitle | Label::TipBody) {
            continue;
        }
        if let Some(w) = session.watch.as_deref() {
            let s = watching(*label, w, &session, now);
            if **t != s {
                **t = s;
            }
            continue;
        }
        let s: String = match (*label, c) {
            (
                Label::SelectTitle
                | Label::SelectMine
                | Label::SelectTeam
                | Label::SelectReroll
                | Label::SelectReady
                | Label::Bench(_),
                _,
            ) => select_text(*label, &session),
            (Label::Center, _) => match session.phase {
                Phase::Connecting => "Connecting...".into(),
                Phase::SigningIn => "Sign in with your wallet...".into(),
                Phase::Watching | Phase::Selecting => String::new(),
                Phase::Lobby => "In the lobby".into(),
                Phase::Reconnecting => "Reconnecting...".into(),
                Phase::Queued => format!(
                    "Finding a match: {} waiting, bots take the empty seats in {} s",
                    session.waiting.max(1),
                    session.fill_in
                ),
                Phase::Failed => session.error.clone().unwrap_or_else(|| "Disconnected".into()),
                Phase::Over => match (session.winner, c) {
                    // Neither team: the server stopped the match (`el_server::lobby::NO_CONTEST`).
                    (Some(w), _) if w > 1 => "No contest: the match was stopped".into(),
                    (Some(w), Some(c)) if w == c.team => "VICTORY".into(),
                    (Some(_), Some(_)) => "DEFEAT".into(),
                    _ => "The match is over".into(),
                },
                Phase::Playing => match c {
                    Some(c) if !c.started() => "Taking the field...".into(),
                    Some(c) if !c.pred.units[c.seat].alive => {
                        let left = c.pred.champs[c.seat].dead_until.saturating_sub(c.pred.tick);
                        format!("Back in {} s", left.div_ceil(TICK_HZ))
                    }
                    // The headliner, as the match starts.
                    Some(c) if c.world.tick < HEADLINER_SHOWN * TICK_HZ => {
                        let h = c.pred.headliner();
                        format!("{}\n{}", h.name, h.blurb)
                    }
                    _ => String::new(),
                },
            },
            (Label::Clock, Some(c)) => mmss(c.world.tick),
            (Label::KillsDawn | Label::KillsDusk, Some(_)) => {
                let team = u8::from(*label == Label::KillsDusk);
                (0..SEATS)
                    .filter(|&s| team_of_seat(s) == team)
                    .map(|s| u32::from(session.kd[s].0))
                    .sum::<u32>()
                    .to_string()
            }
            (Label::Ping, Some(c)) => {
                format!("{:.0} ms   seat {}   {}", c.clock.rtt * 1_000.0, c.seat, session.name(c.seat))
            }
            (Label::Feed, _) => feed_text(&session, now),
            (Label::Name, Some(c)) => {
                let ch = &c.pred.champs[c.seat];
                let points =
                    if ch.points > 0 { format!("   +{} (Ctrl+key)", ch.points) } else { String::new() };
                format!("{}{points}", ch.def().name)
            }
            (Label::Kda, Some(c)) => {
                let (k, d) = session.kd[c.seat];
                format!("{k} / {d}   {} cs", c.pred.champs[c.seat].last_hits)
            }
            (Label::Level, Some(c)) => c.pred.champs[c.seat].level.to_string(),
            (Label::Initials, Some(c)) => initials(c.pred.champs[c.seat].def),
            (Label::Gold | Label::ShopGold, Some(c)) => {
                format!("{:.0} g", c.pred.champs[c.seat].gold.floor())
            }
            (Label::Hp, Some(c)) => {
                let u = &c.pred.units[c.seat];
                format!("{:.0} / {:.0}", u.hp.max(0.0), u.stats.max_hp)
            }
            (Label::Mana, Some(c)) => {
                let u = &c.pred.units[c.seat];
                format!("{:.0} / {:.0}", u.mana.max(0.0), u.stats.max_mana)
            }
            (Label::Key(k), _) => crate::input::key_names(controls.wasd)[k].into(),
            (Label::AbilityName(k), Some(c)) => {
                let a = ability(c.pred.champs[c.seat].def, k as u8);
                // The name's last word, to fit the slot.
                a.name.split_whitespace().last().unwrap_or("").chars().take(9).collect()
            }
            (Label::Cooldown(k), Some(c)) => {
                let ch = &c.pred.champs[c.seat];
                let rank = if k < 4 { ch.ranks[k] } else { 1 };
                let left = ch.ready[k].saturating_sub(c.pred.tick);
                if rank > 0 && left > 0 {
                    let secs = left as f32 / TICK_HZ as f32;
                    if secs < 1.0 { format!("{secs:.1}") } else { format!("{:.0}", secs.ceil()) }
                } else {
                    String::new()
                }
            }
            (Label::CardTier(k), Some(c)) => {
                let a = &AUGMENTS[usize::from(c.pred.champs[c.seat].screen.offers[k]) % AUGMENTS.len()];
                tips::tier_name(a.tier).to_uppercase()
            }
            (Label::CardName(k), Some(c)) => {
                AUGMENTS[usize::from(c.pred.champs[c.seat].screen.offers[k]) % AUGMENTS.len()].name.into()
            }
            (Label::CardText(k), Some(c)) => {
                tips::augment_text(c.pred.champs[c.seat].screen.offers[k] % AUGMENTS.len() as u8)
            }
            (Label::CardKey(k), _) => {
                let reroll = if controls.wasd { "Ctrl" } else { "Shift" };
                format!("[{}]   {reroll}+{} rerolls", k + 1, k + 1)
            }
            (Label::Item(k), Some(c)) => {
                let it = c.pred.champs[c.seat].items[k];
                if it == items::NONE { String::new() } else { ITEMS[usize::from(it)].name.into() }
            }
            (Label::Shop(k), _) => ITEMS[k].name.into(),
            (Label::ShopCost(k), _) => ITEMS[k].cost.to_string(),
            (Label::Vote, Some(c)) => {
                let v = &c.world.surrender[usize::from(c.team)];
                let mine = 1u8 << (c.seat % el_sim::units::TEAM_SIZE);
                let said = if v.yes & mine != 0 {
                    "   (you: yes)"
                } else if v.no & mine != 0 {
                    "   (you: no)"
                } else {
                    ""
                };
                format!("Surrender?  {} yes, {} no{said}", v.yes.count_ones(), v.no.count_ones())
            }
            (Label::CardReroll, _) => "Reroll".into(),
            (Label::VoteYes, _) => "Yes".into(),
            (Label::VoteNo, _) => "No".into(),
            (Label::Surrender, _) => "Surrender".into(),
            (Label::ScoreHead, Some(c)) => {
                let h = c.pred.headliner();
                format!("{}: {}", h.name, h.blurb)
            }
            (Label::ScoreTeam(t), Some(c)) => score_rows(t, |s| {
                let (kills, deaths) = session.kd[s];
                let m = if s == c.seat { &c.pred } else { &c.world };
                Row {
                    name: session.name(s).to_string(),
                    def: m.champs[s].def,
                    level: m.champs[s].level,
                    kills,
                    deaths,
                    you: s == c.seat,
                }
            }),
            (Label::AllyName(i), Some(c)) => match ally_seats(c).nth(i) {
                Some(s) => {
                    let name = session.name(s);
                    let dead = c.world.champs[s].dead_until.saturating_sub(c.world.tick);
                    if c.world.units[s].alive || dead == 0 {
                        name.to_string()
                    } else {
                        format!("{name}  {}s", dead.div_ceil(TICK_HZ))
                    }
                }
                None => String::new(),
            },
            (Label::AllyLevel(i), Some(c)) => {
                ally_seats(c).nth(i).map_or(String::new(), |s| c.world.champs[s].level.to_string())
            }
            (Label::AllyInitials(i), Some(c)) => {
                ally_seats(c).nth(i).map_or(String::new(), |s| initials(c.world.champs[s].def))
            }
            _ => String::new(),
        };
        if **t != s {
            **t = s;
        }
    }
}

/// Text colours that say something: an ability not learned, an item too dear, a dead ally.
fn colors(session: Res<Session>, mut q: Query<(&Label, &mut TextColor)>) {
    let Some(c) = session.client.as_deref() else { return };
    let ch = &c.pred.champs[c.seat];
    for (label, mut color) in &mut q {
        let want = match *label {
            Label::AbilityName(k) => {
                if k < 4 && ch.ranks[k] == 0 {
                    Color::srgb(0.35, 0.37, 0.42)
                } else {
                    DIM
                }
            }
            Label::Shop(k) => {
                if ch.gold >= ITEMS[k].cost as f32 {
                    TEXT
                } else {
                    DIM
                }
            }
            Label::ShopCost(k) => {
                if ch.gold >= ITEMS[k].cost as f32 {
                    GOLD
                } else {
                    SHORT
                }
            }
            Label::AllyName(i) => match ally_seats(c).nth(i) {
                Some(s) if !c.world.units[s].alive => DIM,
                _ => TEXT,
            },
            Label::CardTier(k) => {
                tier_color(AUGMENTS[usize::from(ch.screen.offers[k]) % AUGMENTS.len()].tier)
            }
            _ => continue,
        };
        color.set_if_neq(TextColor(want));
    }
}

fn tier_color(t: Tier) -> Color {
    match t {
        Tier::Silver => Color::srgb(0.78, 0.82, 0.88),
        Tier::Gold => GOLD,
        Tier::Prismatic => Color::srgb(0.86, 0.52, 0.98),
    }
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
            Fill::AllyHp(i) => match ally_seats(c).nth(i) {
                Some(s) if c.world.units[s].alive => {
                    c.world.units[s].hp / c.world.units[s].stats.max_hp.max(1.0)
                }
                _ => 0.0,
            },
        };
        // Written only when the percent moves: a node changed is a layout done again.
        let pct = percent((share.clamp(0.0, 1.0) * 100.0) as i32);
        match *f {
            Fill::Cooldown(_) if node.height != pct => node.height = pct,
            Fill::Cooldown(_) => {}
            _ if node.width != pct => node.width = pct,
            _ => {}
        }
    }
}

/// An ability's border: gold when it's ready, grey cooling down, blue short of mana, dark not
/// learned.
fn slots(session: Res<Session>, mut q: Query<(&AbilitySlot, &mut BorderColor)>) {
    let Some(c) = session.client.as_deref() else { return };
    let u = &c.pred.units[c.seat];
    let ch = &c.pred.champs[c.seat];
    for (slot, mut border) in &mut q {
        let k = slot.0;
        let rank = if k < 4 { ch.ranks[k] } else { 1 };
        let cost = ability(ch.def, k as u8).cost[usize::from(rank.max(1) - 1).min(4)];
        let want = if rank == 0 {
            Color::srgb(0.16, 0.17, 0.20)
        } else if ch.ready[k] > c.pred.tick {
            Color::srgb(0.30, 0.31, 0.34)
        } else if cost > u.mana {
            MANA
        } else {
            GOLD
        };
        border.set_if_neq(BorderColor::all(want));
    }
}

fn pips(session: Res<Session>, mut q: Query<(&Pip, &mut BackgroundColor)>) {
    let Some(c) = session.client.as_deref() else { return };
    let ch = &c.pred.champs[c.seat];
    for (p, mut bg) in &mut q {
        let want = if p.1 < ch.ranks[p.0] { GOLD } else { Color::srgb(0.20, 0.21, 0.25) };
        bg.set_if_neq(BackgroundColor(want));
    }
}

fn level_ups(session: Res<Session>, mut q: Query<(&LevelUp, &mut Visibility)>) {
    let c = session.client.as_deref().filter(|_| session.phase == Phase::Playing);
    for (l, mut vis) in &mut q {
        let show = c.is_some_and(|c| may_level(c, l.0));
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
}

fn card_frames(session: Res<Session>, mut q: Query<(&CardFrame, &mut BorderColor)>) {
    let Some(c) = session.client.as_deref() else { return };
    let screen = &c.pred.champs[c.seat].screen;
    for (f, mut border) in &mut q {
        let tier = AUGMENTS[usize::from(screen.offers[f.0]) % AUGMENTS.len()].tier;
        border.set_if_neq(BorderColor::all(tier_color(tier)));
    }
}

/// The portraits in their champions' colours, an ally's border its life.
fn portraits(session: Res<Session>, mut q: Query<(&Portrait, &mut BackgroundColor, &mut BorderColor)>) {
    let Some(c) = session.client.as_deref() else { return };
    for (p, mut bg, mut border) in &mut q {
        let (def, alive) = match p.0 {
            None => (c.pred.champs[c.seat].def, c.pred.units[c.seat].alive),
            Some(i) => match ally_seats(c).nth(i) {
                Some(s) => (c.world.champs[s].def, c.world.units[s].alive),
                None => continue,
            },
        };
        let base = CHAMPION[usize::from(def) % CHAMPION.len()];
        let want = if alive { shade(base, -0.25) } else { Color::srgb(0.12, 0.12, 0.14) };
        bg.set_if_neq(BackgroundColor(want));
        if p.0.is_some() {
            border.set_if_neq(BorderColor::all(if alive { FRAME } else { Color::srgb(0.25, 0.10, 0.10) }));
        }
    }
}

fn ally_rows(session: Res<Session>, mut q: Query<(&AllyRow, &mut Node)>) {
    let n = session.client.as_deref().map_or(0, |c| ally_seats(c).count());
    for (r, mut node) in &mut q {
        let want = if r.0 < n { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

/// The window a HUD's size is drawn for at scale 1; it grows and shrinks with the window from
/// there, as a MOBA's does with the resolution.
const DESIGNED: Vec2 = Vec2::new(1_600.0, 900.0);

fn scale(window: Single<&Window>, mut ui: ResMut<UiScale>) {
    let size = window.size() / DESIGNED;
    let want = (size.x.min(size.y) * 20.0).round() / 20.0;
    let want = want.clamp(0.7, 1.5);
    if ui.0 != want {
        ui.0 = want;
    }
}

/// A button lighter under the cursor, darker pressed.
fn tints(mut q: Query<(&Interaction, &Tint, &mut BackgroundColor), Changed<Interaction>>) {
    for (i, tint, mut bg) in &mut q {
        bg.0 = match i {
            Interaction::Hovered => shade(tint.0, 0.07),
            Interaction::Pressed => shade(tint.0, -0.03),
            Interaction::None => tint.0,
        };
    }
}

fn panels(session: Res<Session>, controls: Res<Controls>, mut q: Query<(&Panel, &mut Node)>) {
    let c = session.client.as_deref().filter(|c| c.started() && session.phase == Phase::Playing);
    let watching = session.watch.is_some();
    let now = crate::net::now_s();
    let feed = session.feed.iter().any(|l| now - l.at < 12.0);
    for (p, mut node) in &mut q {
        let show = match (*p, c) {
            (Panel::Select, _) => session.phase == Phase::Selecting && session.select.is_some(),
            (Panel::Feed, _) => feed,
            (Panel::Tip, _) => node.display == Display::Flex,
            (Panel::Top, _) if watching => true,
            (Panel::Score, _) if watching => controls.scoreboard,
            (Panel::Top | Panel::Bottom, Some(_)) => true,
            // The shop opens where the allies are.
            (Panel::Allies, Some(c)) => !(controls.shop_open && c.pred.in_shop(c.seat)),
            (Panel::Cards, Some(c)) => c.pred.champs[c.seat].screen.open && c.pred.in_shop(c.seat),
            (Panel::Shop, Some(c)) => controls.shop_open && c.pred.in_shop(c.seat),
            (Panel::Score, Some(_)) => controls.scoreboard,
            (Panel::Vote, Some(c)) => c.world.surrender[usize::from(c.team)].open(),
            (Panel::Surrender, Some(c)) => {
                controls.scoreboard
                    && c.world.tick >= el_sim::ticks(rules::SURRENDER_FROM)
                    && !c.world.surrender[usize::from(c.team)].open()
            }
            _ => false,
        };
        let want = if show { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

/// The tooltip's width (the HUD's pixels).
const TIP_W: f32 = 300.0;

/// The tooltip: what's under the cursor, if it has something to say, just over it.
fn tooltip(
    session: Res<Session>,
    window: Single<&Window>,
    ui: Res<UiScale>,
    hovered: Query<(&Interaction, &Tip, &ComputedNode, &UiGlobalTransform)>,
    mut panel: Query<(&Panel, &mut Node)>,
    mut words: Query<(&Label, &mut Text)>,
) {
    let c = session.client.as_deref().filter(|_| session.phase == Phase::Playing);
    let tip = hovered.iter().find(|(i, ..)| **i != Interaction::None);
    // What it's about: its top left corner and its size, in the HUD's pixels.
    let over = tip.map(|(_, _, n, t)| {
        let (_, _, centre) = t.to_scale_angle_translation();
        ((centre - n.size * 0.5) * n.inverse_scale_factor, n.size * n.inverse_scale_factor)
    });
    let tip = tip.map(|(_, t, ..)| *t);
    let said = c.zip(tip).map(|(c, tip)| {
        let ch = &c.pred.champs[c.seat];
        match tip {
            Tip::Ability(k) => tips::ability_tip(ch.def, k as u8, if k < 4 { ch.ranks[k] } else { 1 }),
            Tip::Item(k) if ch.items[k] != items::NONE => {
                let mut s = tips::item_tip(ch.items[k]);
                s.push_str("\nClick in the shop to sell it.");
                s
            }
            Tip::Item(_) => String::new(),
            Tip::Shop(k) => tips::item_tip(k as u8),
        }
    });
    let said = said.filter(|s| !s.is_empty());
    let Some((_, mut node)) = panel.iter_mut().find(|(p, _)| **p == Panel::Tip) else { return };
    let (Some(said), Some(over)) = (said, over) else {
        if node.display != Display::None {
            node.display = Display::None;
        }
        return;
    };
    let (title, body) = said.split_once('\n').unwrap_or((said.as_str(), ""));
    for (label, mut t) in &mut words {
        let want = match label {
            Label::TipTitle => title,
            Label::TipBody => body,
            _ => continue,
        };
        if t.as_str() != want {
            **t = want.to_string();
        }
    }
    // Over it, their left edges together and kept in the window; beside it in the shop, which is
    // at the top.
    let size = window.size() / ui.0;
    let ((at, its), shop) = (over, matches!(tip, Some(Tip::Shop(_))));
    let (top, bottom, left) = if shop {
        (px(at.y), Val::Auto, at.x + its.x + 8.0)
    } else {
        (Val::Auto, px(size.y - at.y + 8.0), at.x)
    };
    let left = px(left.clamp(4.0, (size.x - TIP_W - 4.0).max(4.0)));
    node.display = Display::Flex;
    if node.top != top || node.bottom != bottom || node.left != left {
        node.top = top;
        node.bottom = bottom;
        node.left = left;
    }
}

fn dots(
    session: Res<Session>,
    drawn: Res<Drawn>,
    mut q: Query<(&Dot, &mut Node, &mut BackgroundColor, &mut BorderColor, &mut Visibility)>,
) {
    let me = session.client.as_deref().map(|c| c.seat);
    for (d, mut node, mut bg, mut border, mut vis) in &mut q {
        match drawn.units[d.0] {
            // Only what moved is written: a node changed is a layout done again.
            Some(u) => {
                vis.set_if_neq(Visibility::Inherited);
                let (left, top) = (
                    percent((u.pos.x / MAP_W * 100.0) as i32),
                    percent(((1.0 - u.pos.y / MAP_H) * 100.0) as i32),
                );
                if node.left != left || node.top != top {
                    node.left = left;
                    node.top = top;
                }
                let mine = Some(d.0) == me;
                bg.set_if_neq(BackgroundColor(TEAM[usize::from(u.team.min(1))]));
                border.set_if_neq(BorderColor::all(if mine {
                    GOLD
                } else {
                    Color::srgba(0.0, 0.0, 0.0, 0.9)
                }));
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// The camera's view on the minimap: the ground under the window's middle row (its width) and
/// middle column (its depth).
fn view_box(
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut q: Query<(&mut Node, &mut Visibility), With<ViewBox>>,
) {
    let Ok((mut node, mut vis)) = q.single_mut() else { return };
    let (cam, at) = *camera;
    let size = window.size();
    let ground = |p: Vec2| {
        cam.viewport_to_world(at, p)
            .ok()
            .and_then(|ray| ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y)))
            .map(map_point)
    };
    let (Some(l), Some(r), Some(t), Some(b)) = (
        ground(Vec2::new(0.0, size.y * 0.5)),
        ground(Vec2::new(size.x, size.y * 0.5)),
        ground(Vec2::new(size.x * 0.5, 0.0)),
        ground(Vec2::new(size.x * 0.5, size.y)),
    ) else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    vis.set_if_neq(Visibility::Inherited);
    let lo = Vec2::new(l.x.max(0.0), b.y.max(0.0));
    let hi = Vec2::new(r.x.min(MAP_W), t.y.min(MAP_H));
    let (left, top, width, height) = (
        percent(lo.x / MAP_W * 100.0),
        percent((1.0 - hi.y / MAP_H) * 100.0),
        percent(((hi.x - lo.x) / MAP_W * 100.0).max(0.0)),
        percent(((hi.y - lo.y) / MAP_H * 100.0).max(0.0)),
    );
    if node.left != left || node.top != top || node.width != width || node.height != height {
        node.left = left;
        node.top = top;
        node.width = width;
        node.height = height;
    }
}

fn minimap(
    mouse: Res<ButtonInput<MouseButton>>,
    q: Query<&RelativeCursorPosition, With<Minimap>>,
    mut rig: ResMut<crate::camera::Rig>,
    mut controls: ResMut<Controls>,
    session: Res<Session>,
) {
    let Some(n) = q.iter().find(|r| r.cursor_over()).and_then(|r| r.normalized) else { return };
    // (-0.5, -0.5) is the top left: the map's far end of its first half.
    let at = Vec2::new((n.x + 0.5) * MAP_W, (0.5 - n.y) * MAP_H);
    if mouse.pressed(MouseButton::Left) {
        // A look while it's held: the camera stays there if it's free, and goes back if locked.
        rig.peek = Some(at);
    }
    if mouse.just_pressed(MouseButton::Right) && session.phase == Phase::Playing {
        controls.pending.push(Order::Move { to: at });
        controls.marker = Some((at, crate::net::now_s(), false));
    }
}

/// A button pressed: its order, or champion select's message.
fn press(
    q: Query<(&Interaction, &Act), Changed<Interaction>>,
    session: Res<Session>,
    mut controls: ResMut<Controls>,
) {
    for (i, act) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        let order = match *act {
            Act::Pick(card) => Order::PickAugment { card },
            Act::Reroll(card) => Order::Reroll { card },
            Act::Buy(item) => Order::Buy { item },
            Act::Sell(inv) => Order::Sell { inv },
            Act::Level(slot) => Order::LevelSkill { slot },
            Act::Vote(yes) => Order::Surrender { yes },
            Act::Swap(k) => {
                if let Some(&champ) = session.select.as_ref().and_then(|s| s.bench.get(k)) {
                    controls.lobby.push(ToServer::Swap { champ });
                }
                continue;
            }
            Act::RerollChampion => {
                controls.lobby.push(ToServer::Reroll);
                continue;
            }
            Act::Ready => {
                controls.lobby.push(ToServer::Ready);
                continue;
            }
        };
        controls.pending.push(order);
    }
}

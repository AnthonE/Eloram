//! What the player reads, laid out as MOBA players' eyes expect it (champion select and the screens
//! around a match are `hud_select.rs` and `hud_lobby.rs`): at the bottom their portrait
//! and level, the abilities (the key on each, the cooldown counted down over it, the ranks under it,
//! a "+" over it while a point may go there, its border blue when the mana's short), health and
//! mana, the items and the gold; the score and the clock at the top; the allies down the left (F2
//! to F5 look at them); the kill feed at the right; the minimap at the bottom right with the
//! camera's view on it; a tooltip over anything with something to say (an ability, an item, a card);
//! the augment cards while a screen is open and the shop may be used; the shop (P); the scoreboard
//! (Tab), with a button to open a vote to surrender from 8:00; the vote while it's open; and the
//! line in the middle that says what's happening (taking the field, the headliner). All of it read
//! from the prediction, so it moves when the player does.

use bevy::prelude::*;
use bevy::ui::{
    AngularColorStop, BackgroundGradient, BoxShadow, ConicGradient, LinearGradient, RelativeCursorPosition,
    UiGlobalTransform, UiPosition, UiTransform,
};
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

pub(crate) const PANEL: Color = Color::srgba(0.035, 0.045, 0.07, 0.90);
pub(crate) const PANEL_TOP: Color = Color::srgba(0.07, 0.085, 0.12, 0.94);
/// The frames' bronze, and a slot's well.
pub(crate) const FRAME: Color = Color::srgb(0.47, 0.39, 0.24);
pub(crate) const WELL: Color = Color::srgb(0.07, 0.08, 0.11);
/// The trinket's well: a little warmer than an item's.
const TREAT_WELL: Color = Color::srgb(0.16, 0.11, 0.07);
pub(crate) const TRIM: Color = Color::srgb(0.78, 0.64, 0.36);
/// The bronze, faint, for what floats over the map rather than docks.
pub(crate) const FRAME_SOFT: Color = Color::srgba(0.47, 0.39, 0.24, 0.55);
pub(crate) const TEXT: Color = Color::srgb(0.92, 0.93, 0.96);
pub(crate) const DIM: Color = Color::srgb(0.56, 0.59, 0.66);
pub(crate) const GOLD: Color = Color::srgb(0.98, 0.82, 0.36);
const MANA: Color = Color::srgb(0.30, 0.55, 0.98);
const SHORT: Color = Color::srgb(0.90, 0.40, 0.36);
const HP: [Color; 2] = [Color::srgb(0.20, 0.62, 0.24), Color::srgb(0.38, 0.86, 0.36)];
/// Seconds the headliner stays in the middle of the screen as a match starts.
const HEADLINER_SHOWN: u32 = 12;
/// The minimap's square, and the map strip turned in it (the map's 4:1 turned 45° spans the
/// square: (w + h) / √2 = side).
const MINI: f32 = 224.0;
const MINI_W: f32 = MINI * std::f32::consts::SQRT_2 * 0.8;
const MINI_H: f32 = MINI * std::f32::consts::SQRT_2 * 0.2;
/// The stats beside the portrait: what each is called there.
const STATS: [&str; 8] = ["AD", "AP", "AR", "MR", "AS", "AH", "MS", "RG"];
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
    /// An inventory slot's active's seconds left (0 to 5), or the trinket's (6).
    ItemCd(usize),
    /// The key over an inventory slot whose item has an active.
    ItemKey(usize),
    /// The trinket's treats.
    Treats,
    Shop(usize),
    ShopCost(usize),
    ShopGold,
    ScoreHead,
    /// A scoreboard row's words: team, row, and which.
    Score(u8, usize, ScoreField),
    Vote,
    VoteYes,
    VoteNo,
    Surrender,
    AllyName(usize),
    AllyLevel(usize),
    AllyInitials(usize),
    TipTitle,
    TipBody,
    /// One of the stats beside the portrait ([`STATS`]).
    Stat(usize),
    /// The seconds to respawn, over the portrait.
    Respawn,
    /// The Recommended row's title.
    ShopRec,
}

/// A bar's fill.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Fill {
    Hp,
    Mana,
    Xp,
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
    /// The minimap, while there's a match on it.
    Minimap,
    /// The team's vote to surrender, while it's open.
    Vote,
    /// The button that opens one, with the scoreboard, from the time one may.
    Surrender,
    Tip,
}

/// What pressing a button does.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
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
    /// Into the queue (from the lobby, or again once a match is over), or out of it.
    Queue,
    Leave,
    /// The page loaded again (after the link failed for good).
    Reload,
}

/// What a tooltip over this says.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tip {
    Ability(usize),
    Item(usize),
    Shop(usize),
    Trinket,
    /// Champion select: ability `.0` of the champion the player holds there.
    Pick(usize),
}

/// A button's colour at rest: lighter hovered, darker pressed.
#[derive(Component, Clone, Copy)]
pub(crate) struct Tint(pub(crate) Color);

/// An ability's slot (its border says ready, cooling down, short of mana or not learned).
#[derive(Component)]
struct AbilitySlot(usize);

/// An ability's icon (`el_client_core::icons`), over its name.
#[derive(Component)]
struct AbilityIcon(usize);

/// The icons painted so far, by champion and slot.
#[derive(Resource, Default)]
pub(crate) struct Icons(pub(crate) std::collections::HashMap<(u8, u8), Handle<Image>>);

/// An item's icon: in an inventory slot, on the shop's tile for it, or on a scoreboard row (team,
/// row, slot).
#[derive(Component, Clone, Copy)]
enum ItemIcon {
    Held(usize),
    Shop(usize),
    Score(u8, usize, usize),
}

/// A scoreboard row's portrait: in its champion's colour, grey while it's dead.
#[derive(Component)]
struct ScoreBadge(u8, usize);

/// A scoreboard row's item slot, shown only where the items are known.
#[derive(Component)]
struct ScoreWell(u8, usize);

/// A tile of the shop's Recommended row: which of the four, and its icon, name and cost.
#[derive(Component)]
struct RecTile {
    j: usize,
    icon: Entity,
    name: Entity,
    cost: Entity,
}

/// The gold mark on item `.0`'s tile while it's recommended.
#[derive(Component)]
struct RecMark(usize);

/// The items' icons painted so far, in `ITEMS`' order.
#[derive(Resource, Default)]
struct ItemIcons(Vec<Option<Handle<Image>>>);

/// A portrait's live picture of its champion (`portrait.rs`'s view: 0 the player's, then the allies).
#[derive(Component)]
struct PortraitPicture(usize);

/// An ability's cooldown's sweep.
#[derive(Component)]
struct Sweep(usize);

/// The flash over an ability as it comes off cooldown.
#[derive(Component)]
struct ReadyFlash(usize);

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

/// An ally's ultimate's orb.
#[derive(Component)]
struct AllyUlt(usize);

/// An ally's row, shown while there's an ally for it.
#[derive(Component)]
struct AllyRow(usize);

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
        app.init_resource::<Icons>()
            .init_resource::<ItemIcons>()
            .add_systems(PostStartup, setup)
            .add_systems(
                Update,
                (
                    (press, minimap).after(crate::net::NetSet::Receive).before(crate::net::NetSet::Send),
                    (texts, fills, panels, dots, view_box, tooltip).after(crate::net::NetSet::Send),
                    (scale, tints, slots, pips, level_ups, card_frames, portraits, ally_rows, colors)
                        .after(crate::net::NetSet::Send),
                    (ability_icons, item_icons, portrait_picture, rec_tiles, ally_ults, sweeps, score_badges)
                        .after(crate::net::NetSet::Send),
                ),
            );
    }
}

pub(crate) fn text(s: &str, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color))
}

/// A tight shadow under words, to read them over the bright ground.
/// An item slot's cooldown: its seconds over a shade, both empty while it's ready (`Label::ItemCd`).
fn item_cd(k: usize) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        children![(text("", 15.0, TEXT), outline(), Label::ItemCd(k))],
    )
}

pub(crate) fn outline() -> TextShadow {
    TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) }
}

/// A soft shadow under a panel.
pub(crate) fn shadow() -> BoxShadow {
    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.55), px(0), px(4), px(0), px(14))
}

/// A vertical gradient, `top` to `bottom`.
pub(crate) fn down(top: Color, bottom: Color) -> BackgroundGradient {
    BackgroundGradient(vec![LinearGradient::to_bottom(vec![top.into(), bottom.into()]).into()])
}

/// A framed panel's look. It takes the cursor: a click on it is the HUD's, not the map's.
pub(crate) fn framed() -> (BorderColor, BackgroundGradient, BoxShadow, Interaction) {
    (BorderColor::all(FRAME), down(PANEL_TOP, PANEL), shadow(), Interaction::None)
}

/// A framed panel docked at the screen's bottom: its top edge brighter, so the bottom's panels
/// read as one set.
pub(crate) fn docked() -> (BorderColor, BackgroundGradient, BoxShadow, Interaction) {
    (
        BorderColor { top: TRIM, ..BorderColor::all(FRAME) },
        down(PANEL_TOP, PANEL),
        shadow(),
        Interaction::None,
    )
}

/// A colour a little lighter (`by` > 0) or darker.
pub(crate) fn shade(c: Color, by: f32) -> Color {
    let s = c.to_srgba();
    Color::srgba(
        (s.red + by).clamp(0.0, 1.0),
        (s.green + by).clamp(0.0, 1.0),
        (s.blue + by).clamp(0.0, 1.0),
        s.alpha,
    )
}

/// The first two letters of a champion's name, for its portrait.
pub(crate) fn initials(def: u8) -> String {
    CHAMPIONS
        .get(usize::from(def))
        .map_or(String::new(), |d| d.name.chars().take(2).collect::<String>().to_uppercase())
}

/// A bar this tall (px) or less is filled in two bands, not a gradient (`bar`).
const THIN: f32 = 8.0;

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
    let node = Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(0),
        height: percent(100),
        width: percent(100),
        ..default()
    };
    let f = if h <= THIN {
        // A thin bar's gradient is a few rows: two bands of it, its top quarter's and its bottom
        // quarter's, look the same and are drawn with the plain nodes round them (a gradient is
        // a pipeline of its own, so a draw call, and the next node another).
        let (top, bottom) =
            (fill_color[1].mix(&fill_color[0], 0.25), fill_color[1].mix(&fill_color[0], 0.75));
        let band = Node { border: UiRect::top(px((h * 0.5).round())), ..node };
        commands.spawn((band, BorderColor::all(top), BackgroundColor(bottom), fill)).id()
    } else {
        commands.spawn((node, down(fill_color[1], fill_color[0]), fill)).id()
    };
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

pub(crate) const BUTTON: Color = Color::srgb(0.11, 0.12, 0.16);
pub(crate) const BUTTON_GOLD: Color = Color::srgb(0.30, 0.24, 0.10);

#[allow(clippy::too_many_lines)]
fn setup(mut commands: Commands, floor: Res<FloorImage>, portrait: Res<crate::portrait::PortraitImage>) {
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
                border: UiRect::new(px(1), px(1), px(0), px(2)),
                border_radius: BorderRadius::px(0.0, 0.0, 10.0, 10.0),
                ..default()
            },
            // Docked at the top: its bright edge is its bottom.
            BorderColor { bottom: TRIM, ..BorderColor::all(FRAME) },
            down(PANEL_TOP, PANEL),
            shadow(),
            Interaction::None,
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
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(6)),
                    max_width: px(360),
                    display: Display::None,
                    ..default()
                },
                BorderColor::all(FRAME_SOFT),
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
    bottom(&mut commands, &portrait);
    allies(&mut commands, &portrait);
    // The minimap, bottom right, in a square frame: the map turned as the camera turns it, the lane
    // from the bottom left to the top right as ARAM's is. A 4:1 strip turned 45° fills a square.
    let mini = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px((MINI - MINI_W) * 0.5),
                top: px((MINI - MINI_H) * 0.5),
                width: px(MINI_W),
                height: px(MINI_H),
                ..default()
            },
            UiTransform { rotation: Rot2::radians(-crate::camera::VIEW.yaw), ..UiTransform::IDENTITY },
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
    // The square the turned map sits in, its corners the abyss's dark; the view box is cut at it.
    let square = commands
        .spawn((
            Node { width: px(MINI), height: px(MINI), overflow: Overflow::clip(), ..default() },
            BackgroundColor(Color::srgb(0.02, 0.03, 0.05)),
        ))
        .add_child(mini)
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0),
                right: px(0),
                padding: UiRect::all(px(4)),
                border: UiRect::new(px(2), px(0), px(2), px(0)),
                border_radius: BorderRadius::px(10.0, 0.0, 0.0, 0.0),
                display: Display::None,
                ..default()
            },
            docked(),
            Panel::Minimap,
        ))
        .add_child(square);
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
                        min_width: px(420),
                        ..default()
                    },
                    BorderColor::all(TEAM[usize::from(t)]),
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
                    children![text(if t == 0 { "DAWN" } else { "DUSK" }, 14.0, TEAM[usize::from(t)])],
                ))
                .with_children(|team| {
                    for j in 0..el_sim::units::TEAM_SIZE {
                        score_row(team, t, j);
                    }
                })
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
fn bottom(commands: &mut Commands, portrait: &crate::portrait::PortraitImage) {
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
    // The champion's initials, and over them its model drawn live (`portrait.rs`), cut to the
    // frame; the badge sits over the frame's corner, outside the cut.
    let face = commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(8)),
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(GOLD),
            BackgroundColor(WELL),
            Portrait(None),
            children![
                (text("", 26.0, TEXT), outline(), Label::Initials),
                (
                    // The time to respawn, over the portrait while dead, as League's.
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    GlobalZIndex(1),
                    children![(
                        text("", 30.0, TEXT),
                        TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.95) },
                        Label::Respawn
                    )],
                ),
                (
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    ImageNode::new(portrait.0[0].clone()),
                    PortraitPicture(0),
                ),
            ],
        ))
        .id();
    let portrait =
        commands.spawn(Node { width: px(66), height: px(66), ..default() }).add_children(&[face, badge]).id();
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
    // The stats, two columns of four, at the panel's left as League's are: a name and its value
    // each, placed on the grid. Every name is spawned before every value, so each size of text
    // is drawn at once (a glyph atlas a size: name, value, name... was a draw call each).
    let place = |i: usize, value: bool| Node {
        grid_row: GridPlacement::start(i as i16 / 2 + 1),
        grid_column: GridPlacement::start((i as i16 % 2) * 2 + 1 + i16::from(value)),
        ..default()
    };
    let names = STATS
        .iter()
        .enumerate()
        .map(|(i, name)| commands.spawn((text(name, 9.0, DIM), place(i, false))).id());
    let mut cells: Vec<Entity> = names.collect();
    cells.extend(
        (0..STATS.len()).map(|i| commands.spawn((text("", 11.0, TEXT), Label::Stat(i), place(i, true))).id()),
    );
    let stats = commands
        .spawn((
            Node {
                display: Display::Grid,
                // A name's 16 px and its value's 34, twice: the cells of 54 they were.
                grid_template_columns: vec![
                    GridTrack::px(16.0),
                    GridTrack::px(34.0),
                    GridTrack::px(16.0),
                    GridTrack::px(34.0),
                ],
                align_items: AlignItems::Baseline,
                column_gap: px(4),
                row_gap: px(5),
                padding: UiRect::axes(px(8), px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BorderColor::all(FRAME_SOFT),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
        ))
        .add_children(&cells)
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
                    children![
                        (text("", 9.0, TEXT), TextLayout::justify(Justify::Center), Label::Item(k)),
                        (
                            Node {
                                position_type: PositionType::Absolute,
                                width: percent(100),
                                height: percent(100),
                                ..default()
                            },
                            ImageNode::default(),
                            Visibility::Hidden,
                            ItemIcon::Held(k),
                        ),
                        item_cd(k),
                        (
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(2),
                                bottom: px(0),
                                ..default()
                            },
                            children![(text("", 9.0, GOLD), outline(), Label::ItemKey(k))],
                        ),
                    ],
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
                flex_grow: 1.0,
                padding: UiRect::axes(px(6), px(3)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            children![(text("", 15.0, GOLD), outline(), Label::Gold)],
        ))
        .id();
    // The trinket's slot beside the gold: its treats, and the key that eats one.
    let trinket = commands
        .spawn((
            Button,
            Node {
                width: px(38),
                height: px(26),
                padding: UiRect::left(px(8)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(5)),
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(Color::srgb(0.42, 0.32, 0.18)),
            BackgroundColor(TREAT_WELL),
            Tip::Trinket,
            children![
                (text("", 11.0, TEXT), outline(), Label::Treats),
                item_cd(el_proto::order::TRINKET as usize),
                (
                    Node { position_type: PositionType::Absolute, left: px(3), top: px(1), ..default() },
                    children![(text("4", 9.0, GOLD), outline())],
                ),
            ],
        ))
        .id();
    let gold = commands
        .spawn(Node { column_gap: px(4), align_items: AlignItems::Center, ..default() })
        .add_children(&[trinket, gold])
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
                border: UiRect::new(px(1), px(1), px(2), px(0)),
                border_radius: BorderRadius::px(12.0, 12.0, 0.0, 0.0),
                ..default()
            },
            docked(),
        ))
        .add_children(&[stats, left, middle, right])
        .id();
    commands
        .spawn((
            // Centred on the screen, as League's is: the eye rests on the abilities.
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0),
                left: px(0),
                right: px(0),
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
            // Flat: the icon is opaque and covers it all, and a gradient is a draw call of its
            // own.
            BackgroundColor(Color::srgb(0.11, 0.12, 0.17)),
            Interaction::None,
            AbilitySlot(k),
            Tip::Ability(k),
            children![
                (text("", 9.0, DIM), TextLayout::justify(Justify::Center), Label::AbilityName(k)),
                (
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    ImageNode::default(),
                    Visibility::Hidden,
                    AbilityIcon(k),
                ),
                (
                    // The cooldown: a shade swept away clockwise from twelve, as League's.
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundGradient::default(),
                    Sweep(k),
                ),
                (
                    // A flash as it comes back.
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    ReadyFlash(k),
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
fn allies(commands: &mut Commands, portrait: &crate::portrait::PortraitImage) {
    let rows: Vec<Entity> = (0..ALLIES)
        .map(|i| {
            // The initials, and over them the ally's model drawn live, cut to the frame; the level
            // over the frame's corner, outside the cut.
            let face = commands
                .spawn((
                    Node {
                        width: percent(100),
                        height: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::all(px(6)),
                        overflow: Overflow::clip(),
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
                                width: percent(100),
                                height: percent(100),
                                ..default()
                            },
                            ImageNode::new(portrait.0[1 + i].clone()),
                            PortraitPicture(1 + i),
                        ),
                    ],
                ))
                .id();
            let level = commands
                .spawn((
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
                ))
                .id();
            // The ultimate: an orb at the top right, lit while it's ready, as League's team frames.
            let ult = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(-4),
                        top: px(-4),
                        width: px(11),
                        height: px(11),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(percent(50)),
                        ..default()
                    },
                    BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.9)),
                    BackgroundColor(Color::srgb(0.18, 0.18, 0.2)),
                    BoxShadow::new(Color::NONE, px(0), px(0), px(0), px(6)),
                    Visibility::Hidden,
                    AllyUlt(i),
                ))
                .id();
            let portrait = commands
                .spawn(Node { width: px(38), height: px(38), ..default() })
                .add_children(&[face, level, ult])
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
                        border: UiRect::new(px(0), px(1), px(1), px(1)),
                        border_radius: BorderRadius::px(0.0, 8.0, 8.0, 0.0),
                        ..default()
                    },
                    BorderColor::all(FRAME_SOFT),
                    BackgroundGradient(vec![
                        LinearGradient::to_right(vec![
                            Color::srgba(0.05, 0.06, 0.09, 0.85).into(),
                            Color::srgba(0.02, 0.03, 0.05, 0.6).into(),
                        ])
                        .into(),
                    ]),
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
/// A tile of the shop: the item's icon, its name and its cost; a click buys it, a gold mark in its
/// corner while it's one recommended for the champion (`Label::ShopRec`).
fn shop_tile(commands: &mut Commands, k: usize) -> (Entity, [Entity; 3]) {
    let icon = commands
        .spawn((
            Node { width: px(44), height: px(44), border_radius: BorderRadius::all(px(4)), ..default() },
            ImageNode::default(),
            Visibility::Hidden,
            ItemIcon::Shop(k),
        ))
        .id();
    let name =
        commands.spawn((text("", 10.0, TEXT), TextLayout::justify(Justify::Center), Label::Shop(k))).id();
    let cost = commands.spawn((text("", 11.0, GOLD), Label::ShopCost(k))).id();
    let mark = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(3),
                right: px(3),
                width: px(8),
                height: px(8),
                border_radius: BorderRadius::all(px(1)),
                ..default()
            },
            UiTransform { rotation: Rot2::degrees(45.0), ..UiTransform::IDENTITY },
            BackgroundColor(GOLD),
            Visibility::Hidden,
            RecMark(k),
        ))
        .id();
    let tile = commands
        .spawn((
            Button,
            Node {
                width: px(86),
                height: px(92),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(3),
                padding: UiRect::all(px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.22, 0.21, 0.18)),
            BackgroundColor(BUTTON),
            Tint(BUTTON),
            Act::Buy(k as u8),
            Tip::Shop(k),
        ))
        .add_children(&[icon, name, cost, mark])
        .id();
    (tile, [icon, name, cost])
}

/// A titled row of tiles.
fn shop_section(commands: &mut Commands, title: Entity, tiles: &[Entity]) -> Entity {
    let row = commands
        .spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() })
        .add_children(tiles)
        .id();
    commands
        .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() })
        .add_children(&[title, row])
        .id()
}

/// The shop (P), in the middle of the screen as League's: what's recommended for the champion's
/// role first, then every item, the parts and the finished ones apart.
fn shop(commands: &mut Commands) {
    let rec: Vec<Entity> = (0..4)
        .map(|j| {
            let (tile, [icon, name, cost]) = shop_tile(commands, 0);
            commands.entity(tile).insert(RecTile { j, icon, name, cost });
            tile
        })
        .collect();
    let (parts, done): (Vec<usize>, Vec<usize>) = (0..ITEMS.len()).partition(|&k| ITEMS[k].cost < 1_000);
    let parts: Vec<Entity> = parts.into_iter().map(|k| shop_tile(commands, k).0).collect();
    let done: Vec<Entity> = done.into_iter().map(|k| shop_tile(commands, k).0).collect();
    let rec_title = commands.spawn((text("", 12.0, GOLD), Label::ShopRec)).id();
    let parts_title = commands.spawn(text("COMPONENTS", 12.0, DIM)).id();
    let done_title = commands.spawn(text("FINISHED ITEMS", 12.0, DIM)).id();
    let rec = shop_section(commands, rec_title, &rec);
    let parts = shop_section(commands, parts_title, &parts);
    let done = shop_section(commands, done_title, &done);
    let title = commands.spawn(text("SHOP", 16.0, GOLD)).id();
    let gold = commands.spawn((text("", 15.0, GOLD), outline(), Label::ShopGold)).id();
    let head = commands
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .add_children(&[title, gold])
        .id();
    let hint =
        commands.spawn(text("Click to buy. Click an item you hold to sell it. P closes.", 11.0, DIM)).id();
    let rule = || {
        (
            Node { height: px(1), width: percent(100), ..default() },
            BackgroundColor(Color::srgba(0.47, 0.39, 0.24, 0.4)),
        )
    };
    let rule_a = commands.spawn(rule()).id();
    let rule_b = commands.spawn(rule()).id();
    let body = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                width: px(7.0 * 92.0 + 24.0),
                padding: UiRect::all(px(14)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            framed(),
        ))
        .add_children(&[head, hint, rec, rule_a, parts, rule_b, done])
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
            Panel::Shop,
        ))
        .add_child(body);
}

/// Champion `c`'s name, or a dash if there's none to show.
fn champ_name(c: u8) -> &'static str {
    CHAMPIONS.get(usize::from(c)).map_or("-", |d| d.name)
}

pub(crate) fn mmss(ticks: u32) -> String {
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
}

/// What a scoreboard row's words say.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScoreField {
    Initials,
    Name,
    Champion,
    Level,
    Kd,
}

/// The seat of row `j` of team `t`'s on the scoreboard.
fn score_seat(t: u8, j: usize) -> Option<usize> {
    (0..SEATS).filter(|&s| team_of_seat(s) == t).nth(j)
}

/// A scoreboard row's words.
fn score_text(f: ScoreField, r: &Row) -> String {
    match f {
        ScoreField::Initials => initials(r.def),
        ScoreField::Name => r.name.chars().take(16).collect(),
        ScoreField::Champion => champ_name(r.def % CHAMPIONS.len() as u8).into(),
        ScoreField::Level => r.level.to_string(),
        ScoreField::Kd => format!("{} / {}", r.kills, r.deaths),
    }
}

/// A scoreboard row: the champion's portrait, the player and the champion, the level, kills and
/// deaths, and the items (the player's own, or everyone's watching a record: nothing else tells a
/// team what the others hold).
fn score_row(team: &mut ChildSpawnerCommands, t: u8, j: usize) {
    let f = |field| Label::Score(t, j, field);
    team.spawn(Node { align_items: AlignItems::Center, column_gap: px(10), height: px(36), ..default() })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(32),
                    height: px(32),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(5)),
                    ..default()
                },
                BorderColor::all(FRAME),
                BackgroundColor(WELL),
                ScoreBadge(t, j),
                children![(text("", 12.0, TEXT), outline(), f(ScoreField::Initials))],
            ));
            row.spawn(Node { flex_direction: FlexDirection::Column, width: px(130), ..default() })
                .with_children(|who| {
                    who.spawn((text("", 13.0, TEXT), f(ScoreField::Name)));
                    who.spawn((text("", 11.0, DIM), f(ScoreField::Champion)));
                });
            row.spawn((
                Node { width: px(24), justify_content: JustifyContent::Center, ..default() },
                children![(text("", 13.0, GOLD), f(ScoreField::Level))],
            ));
            row.spawn((
                Node { width: px(56), justify_content: JustifyContent::Center, ..default() },
                children![(text("", 13.0, TEXT), f(ScoreField::Kd))],
            ));
            row.spawn(Node { column_gap: px(2), ..default() }).with_children(|items_row| {
                for k in 0..items::SLOTS {
                    items_row.spawn((
                        Node {
                            width: px(22),
                            height: px(22),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                        Visibility::Hidden,
                        ScoreWell(t, j),
                        children![(
                            Node { width: percent(100), height: percent(100), ..default() },
                            ImageNode::default(),
                            Visibility::Hidden,
                            ItemIcon::Score(t, j, k),
                        )],
                    ));
                }
            });
        });
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
        Label::Score(t, j, f) => score_seat(t, j).map_or(String::new(), |s| {
            let ch = &m.champs[s];
            let r = Row {
                name: session.name(s).to_string(),
                def: ch.def,
                level: ch.level,
                kills: ch.kills,
                deaths: ch.deaths,
            };
            score_text(f, &r)
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
            (Label::Center, _) => match session.phase {
                // The screens around a match say these (`hud_lobby.rs`, `hud_select.rs`).
                Phase::Connecting
                | Phase::SigningIn
                | Phase::Lobby
                | Phase::Reconnecting
                | Phase::Queued
                | Phase::Failed
                | Phase::Over
                | Phase::Watching
                | Phase::Selecting => String::new(),
                Phase::Playing => match c {
                    Some(c) if !c.started() => "Taking the field...".into(),
                    // Dead: the time is over the portrait, and the recap says the rest
                    // (`hud_death.rs`).
                    Some(c) if !c.pred.units[c.seat].alive => String::new(),
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
                format!("{:.0} ms", c.clock.rtt * 1_000.0)
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
            (Label::ShopRec, Some(c)) => format!(
                "RECOMMENDED FOR {}",
                el_sim::content::champions::CHAMPIONS[usize::from(c.pred.champs[c.seat].def) % 20]
                    .role
                    .name()
                    .to_uppercase()
            ),
            (Label::Respawn, Some(c)) if !c.pred.units[c.seat].alive => {
                let left = c.pred.champs[c.seat].dead_until.saturating_sub(c.pred.tick);
                if left > 0 { left.div_ceil(TICK_HZ).to_string() } else { String::new() }
            }
            (Label::Stat(i), Some(c)) => {
                let st = &c.pred.units[c.seat].stats;
                match i {
                    0 => format!("{:.0}", st.ad),
                    1 => format!("{:.0}", st.ap),
                    2 => format!("{:.0}", st.armor),
                    3 => format!("{:.0}", st.mr),
                    4 => format!("{:.2}", st.attack_speed),
                    5 => format!("{:.0}", st.haste),
                    6 => format!("{:.0}", st.move_speed),
                    _ => format!("{:.0}", st.range),
                }
            }
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
            (Label::ItemCd(k), Some(c)) => {
                let ch = &c.pred.champs[c.seat];
                let ready = if k < items::SLOTS { ch.active_ready[k] } else { ch.treat_ready };
                let left = ready.saturating_sub(c.pred.tick);
                if left > 0 { format!("{:.0}", (left as f32 / TICK_HZ as f32).ceil()) } else { String::new() }
            }
            (Label::ItemKey(k), Some(c)) => {
                let it = c.pred.champs[c.seat].items[k];
                let active = ITEMS.get(usize::from(it)).is_some_and(|d| d.active.is_some());
                if active { ["1", "2", "3", "5", "6", "7"][k].into() } else { String::new() }
            }
            (Label::Treats, Some(c)) => format!("x{}", c.pred.champs[c.seat].treats),
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
            (Label::Score(t, j, f), Some(c)) => score_seat(t, j).map_or(String::new(), |s| {
                let (kills, deaths) = session.kd[s];
                let m = if s == c.seat { &c.pred } else { &c.world };
                let r = Row {
                    name: if s == c.seat {
                        format!("{} (you)", session.name(s))
                    } else {
                        session.name(s).to_string()
                    },
                    def: m.champs[s].def,
                    level: m.champs[s].level,
                    kills,
                    deaths,
                };
                score_text(f, &r)
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
            Fill::AllyHp(i) => match ally_seats(c).nth(i) {
                Some(s) if c.world.units[s].alive => {
                    c.world.units[s].hp / c.world.units[s].stats.max_hp.max(1.0)
                }
                _ => 0.0,
            },
        };
        // Written only when the percent moves: a node changed is a layout done again.
        let pct = percent((share.clamp(0.0, 1.0) * 100.0) as i32);
        if node.width != pct {
            node.width = pct;
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

/// Each ability slot's icon, for the champion played: dim while it has no rank.
fn ability_icons(
    session: Res<Session>,
    mut icons: ResMut<Icons>,
    mut images: ResMut<Assets<Image>>,
    mut q: Query<(&AbilityIcon, &mut ImageNode, &mut Visibility)>,
    mut names: Query<(&Label, &mut Visibility), Without<AbilityIcon>>,
) {
    let Some(c) = session.client.as_deref() else { return };
    let ch = &c.pred.champs[c.seat];
    // The name is for a slot without its icon: under the icon (opaque) it's never seen, but it
    // would still be drawn, a glyph atlas between two icons, so a draw call a slot.
    for (label, mut vis) in &mut names {
        if matches!(label, Label::AbilityName(_)) {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
    for (icon, mut node, mut vis) in &mut q {
        let k = icon.0;
        let h = icons
            .0
            .entry((ch.def, k as u8))
            .or_insert_with(|| images.add(icon_image(el_client_core::icons::ability_icon(ch.def, k as u8))))
            .clone();
        if node.image != h {
            node.image = h;
        }
        let learned = k >= 4 || ch.ranks[k] > 0;
        let tint = if learned { Color::WHITE } else { Color::srgb(0.32, 0.32, 0.36) };
        if node.color != tint {
            node.color = tint;
        }
        vis.set_if_neq(Visibility::Inherited);
    }
}

/// An icon's picture, `ICON` square, from its painted pixels.
pub(crate) fn icon_image(pixels: Vec<u8>) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let side = el_client_core::icons::ICON as u32;
    Image::new(
        Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Each held item's icon, and the shop's; an empty slot shows nothing.
fn item_icons(
    session: Res<Session>,
    mut icons: ResMut<ItemIcons>,
    mut images: ResMut<Assets<Image>>,
    mut q: Query<(&ItemIcon, &mut ImageNode, &mut Visibility)>,
) {
    if icons.0.is_empty() {
        icons.0 = vec![None; ITEMS.len()];
    }
    let held = session.client.as_deref().map(|c| c.pred.champs[c.seat].items);
    let own = session.client.as_deref().map(|c| c.seat);
    let watched = session.watch.as_deref().map(|w| &w.referee.m);
    for (icon, mut node, mut vis) in &mut q {
        let item = match *icon {
            ItemIcon::Shop(k) => Some(k),
            ItemIcon::Held(k) => held.map(|h| h[k]).filter(|&i| i != items::NONE).map(usize::from),
            ItemIcon::Score(t, j, k) => score_seat(t, j)
                .and_then(|s| match watched {
                    Some(m) => Some(m.champs[s].items[k]),
                    None => held.filter(|_| own == Some(s)).map(|h| h[k]),
                })
                .filter(|&i| i != items::NONE)
                .map(usize::from),
        };
        let Some(item) = item.filter(|&i| i < ITEMS.len()) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let h = icons.0[item]
            .get_or_insert_with(|| images.add(icon_image(el_client_core::icons::item_icon(item as u8))))
            .clone();
        if node.image != h {
            node.image = h;
        }
        vis.set_if_neq(Visibility::Inherited);
    }
}

/// The scoreboard's portraits: each champion's colour, grey while it's dead (by its death timer:
/// an enemy out of sight isn't dead); and the item slots where the items are known (the player's
/// own, or everyone's watching a record).
fn score_badges(
    session: Res<Session>,
    mut q: Query<(&ScoreBadge, &mut BackgroundColor)>,
    mut wells: Query<(&ScoreWell, &mut Visibility)>,
) {
    let (m, own) = match (session.watch.as_deref(), session.client.as_deref()) {
        (Some(w), _) => (&w.referee.m, None),
        (None, Some(c)) => (&c.world, Some(c.seat)),
        (None, None) => return,
    };
    for (b, mut bg) in &mut q {
        let Some(s) = score_seat(b.0, b.1) else { continue };
        let base = CHAMPION[usize::from(m.champs[s].def) % CHAMPION.len()];
        let dead = m.champs[s].dead_until > m.tick;
        let want = if dead { Color::srgb(0.14, 0.14, 0.16) } else { shade(base, -0.25) };
        bg.set_if_neq(BackgroundColor(want));
    }
    for (w, mut v) in &mut wells {
        let known = own.is_none() || score_seat(w.0, w.1) == own;
        v.set_if_neq(if known { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// How long a flash lasts as an ability comes back (s).
const FLASH_SECS: f64 = 0.45;

/// Each ability's cooldown as a shade swept away clockwise, and a flash as it comes back. The
/// whole of a cooldown is the most left seen since it began, so haste is in it.
fn sweeps(
    session: Res<Session>,
    mut full: Local<[u32; 6]>,
    mut back_at: Local<[f64; 6]>,
    mut q: Query<(&Sweep, &mut BackgroundGradient)>,
    mut flashes: Query<(&ReadyFlash, &mut BackgroundColor)>,
) {
    let Some(c) = session.client.as_deref() else { return };
    let ch = &c.pred.champs[c.seat];
    let now = crate::net::now_s();
    let mut share = [0.0f32; 6];
    for k in 0..6 {
        let rank = if k < 4 { ch.ranks[k] } else { 1 };
        let left = if rank == 0 { 0 } else { ch.ready[k].saturating_sub(c.pred.tick) };
        if left == 0 {
            if full[k] > 0 {
                back_at[k] = now;
            }
            full[k] = 0;
        } else {
            full[k] = full[k].max(left);
            share[k] = left as f32 / full[k] as f32;
        }
    }
    for (s, mut g) in &mut q {
        let left = share[s.0];
        // Quantised to a degree, so it's written only when it moves.
        let gone = ((1.0 - left) * 360.0).round() / 360.0 * std::f32::consts::TAU;
        let want = if left <= 0.0 {
            BackgroundGradient::default()
        } else {
            let shade = Color::srgba(0.0, 0.0, 0.0, 0.62);
            BackgroundGradient(vec![
                ConicGradient::new(
                    UiPosition::CENTER,
                    vec![
                        AngularColorStop::new(Color::NONE, 0.0),
                        AngularColorStop::new(Color::NONE, gone),
                        AngularColorStop::new(shade, gone),
                        AngularColorStop::new(shade, std::f32::consts::TAU),
                    ],
                )
                .into(),
            ])
        };
        if *g != want {
            *g = want;
        }
    }
    for (f, mut bg) in &mut flashes {
        let t = ((now - back_at[f.0]) / FLASH_SECS) as f32;
        let a = if (0.0..1.0).contains(&t) { (1.0 - t) * 0.6 } else { 0.0 };
        bg.set_if_neq(BackgroundColor(Color::srgba(1.0, 0.97, 0.85, a)));
    }
}

/// Each ally's ultimate's orb: shown from the level it may be learned, lit while it's ready
/// (`flags::ULT_READY`, which only the ally's team is sent).
fn ally_ults(
    session: Res<Session>,
    mut q: Query<(&AllyUlt, &mut BackgroundColor, &mut BoxShadow, &mut Visibility)>,
) {
    let Some(c) = session.client.as_deref() else { return };
    let now = crate::net::now_s();
    for (a, mut bg, mut glow, mut vis) in &mut q {
        let Some(s) = ally_seats(c).nth(a.0) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let learnable = c.world.champs[s].level >= 6;
        vis.set_if_neq(if learnable { Visibility::Inherited } else { Visibility::Hidden });
        let ready = c.world.units[s].alive
            && c.unit(s, now).is_some_and(|u| u.q.flags & el_proto::unit::flags::ULT_READY != 0);
        let (fill, halo) = if ready {
            (Color::srgb(1.0, 0.82, 0.35), Color::srgba(1.0, 0.75, 0.25, 0.8))
        } else {
            (Color::srgb(0.18, 0.18, 0.2), Color::NONE)
        };
        bg.set_if_neq(BackgroundColor(fill));
        let want = BoxShadow::new(halo, px(0), px(0), px(0), px(6));
        if *glow != want {
            *glow = want;
        }
    }
}

/// The Recommended row's tiles, for the champion's role, and the gold mark on every tile that's
/// recommended.
#[allow(clippy::type_complexity)]
fn rec_tiles(
    session: Res<Session>,
    mut tiles: Query<(&RecTile, &mut Act, &mut Tip)>,
    mut icons: Query<&mut ItemIcon>,
    mut labels: Query<&mut Label>,
    mut marks: Query<(&RecMark, &mut Visibility)>,
) {
    let Some(c) = session.client.as_deref() else { return };
    let def = usize::from(c.pred.champs[c.seat].def) % el_sim::content::champions::CHAMPIONS.len();
    let rec = tips::recommended(el_sim::content::champions::CHAMPIONS[def].role);
    for (t, mut act, mut tip) in &mut tiles {
        let k = usize::from(rec[t.j]);
        if *act != Act::Buy(k as u8) {
            *act = Act::Buy(k as u8);
        }
        if *tip != Tip::Shop(k) {
            *tip = Tip::Shop(k);
        }
        if let Ok(mut i) = icons.get_mut(t.icon) {
            *i = ItemIcon::Shop(k);
        }
        for (e, want) in [(t.name, Label::Shop(k)), (t.cost, Label::ShopCost(k))] {
            if let Ok(mut l) = labels.get_mut(e)
                && *l != want
            {
                *l = want;
            }
        }
    }
    for (m, mut v) in &mut marks {
        v.set_if_neq(if rec.contains(&(m.0 as u8)) { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// The portraits' pictures: grey while their champion's dead; the initials only while it isn't
/// drawn.
fn portrait_picture(
    session: Res<Session>,
    shown: Res<crate::portrait::PortraitShown>,
    mut q: Query<(&PortraitPicture, &mut ImageNode)>,
    mut labels: Query<(&Label, &mut Visibility)>,
) {
    for (l, mut v) in &mut labels {
        let view = match *l {
            Label::Initials => 0,
            Label::AllyInitials(i) => 1 + i,
            _ => continue,
        };
        let drawn = shown.0.get(view).copied().unwrap_or(false);
        v.set_if_neq(if drawn { Visibility::Hidden } else { Visibility::Inherited });
    }
    let Some(c) = session.client.as_deref() else { return };
    for (p, mut node) in &mut q {
        let alive = match p.0 {
            0 => c.pred.units[c.seat].alive,
            v => ally_seats(c).nth(v - 1).is_some_and(|s| c.world.units[s].alive),
        };
        let tint = if alive { Color::WHITE } else { Color::srgb(0.35, 0.35, 0.38) };
        if node.color != tint {
            node.color = tint;
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
    // Once a match is over its scoreboard stays up, under the result (`hud_lobby.rs`).
    let over = session.phase == Phase::Over && session.client.is_some();
    for (p, mut node) in &mut q {
        if *p == Panel::Score {
            let top = percent(if over { 27 } else { 18 });
            if node.top != top {
                node.top = top;
            }
        }
        let show = match (*p, c) {
            (Panel::Score, _) if over => true,
            (Panel::Feed, _) => feed,
            (Panel::Tip, _) => node.display == Display::Flex,
            (Panel::Top | Panel::Minimap, _) if watching => true,
            (Panel::Score, _) if watching => controls.scoreboard,
            (Panel::Top | Panel::Bottom | Panel::Minimap, Some(_)) => true,
            (Panel::Allies, Some(_)) => true,
            // The augment cards, the shop and the scoreboard share the middle: the shop or the
            // scoreboard, while it's open, has it.
            (Panel::Cards, Some(c)) => {
                c.pred.champs[c.seat].screen.open
                    && c.pred.in_shop(c.seat)
                    && !controls.shop_open
                    && !controls.scoreboard
            }
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
    // Champion select's abilities are the champion held there, before there's a match.
    let pick = match (tip, session.select.as_ref().filter(|_| session.phase == Phase::Selecting)) {
        (Some(Tip::Pick(k)), Some(s)) => s.mine().map(|def| tips::ability_tip(def, k as u8, 1)),
        _ => None,
    };
    let said = pick.or_else(|| {
        c.zip(tip).map(|(c, tip)| {
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
                Tip::Trinket => tips::treat_tip(ch.treats),
                Tip::Pick(_) => String::new(),
            }
        })
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

/// The camera's view on the minimap: a box as wide as the ground under the window's middle row and
/// as deep as the ground under its middle column, turned as the camera is (the minimap's scale is
/// the same both ways, so a turn on the map is the same turn on it).
fn view_box(
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut q: Query<(&mut Node, &mut UiTransform, &mut Visibility), With<ViewBox>>,
) {
    let Ok((mut node, mut turn, mut vis)) = q.single_mut() else { return };
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
    let (centre, wide, deep) = ((t + b) * 0.5, l.distance(r), t.distance(b));
    let (left, top, width, height) = (
        percent((centre.x - wide * 0.5) / MAP_W * 100.0),
        percent((1.0 - (centre.y + deep * 0.5) / MAP_H) * 100.0),
        percent(wide / MAP_W * 100.0),
        percent(deep / MAP_H * 100.0),
    );
    if node.left != left || node.top != top || node.width != width || node.height != height {
        node.left = left;
        node.top = top;
        node.width = width;
        node.height = height;
    }
    // The UI turns clockwise; the camera's yaw turns the screen's up clockwise on the map.
    turn.set_if_neq(UiTransform {
        rotation: Rot2::radians(crate::camera::VIEW.yaw),
        ..UiTransform::IDENTITY
    });
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
            Act::Queue => {
                controls.lobby.push(ToServer::Queue);
                continue;
            }
            Act::Leave => {
                controls.lobby.push(ToServer::Leave);
                continue;
            }
            Act::Reload => {
                if let Some(w) = web_sys::window() {
                    let _ = w.location().reload();
                }
                continue;
            }
        };
        controls.pending.push(order);
    }
}

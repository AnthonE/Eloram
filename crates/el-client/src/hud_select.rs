//! Champion select, as a screen of its own over the map: the seconds left and a bar running down
//! under them; the player's team down the left, each seat its champion's badge, name and role and
//! who plays it, the player's own picked out in gold; the champion the player holds in the middle,
//! its name, title and role, and its four abilities with their icons (a tooltip over each says
//! what it does at its first rank); the team's bench under it, a tile a champion that a click takes;
//! and the reroll and ready buttons. ARAM's select: no picking from a list, a reroll sends the
//! champion to the bench and deals another.

use bevy::prelude::*;
use el_sim::content::champions::{CHAMPIONS, ability};
use el_sim::units::{SEATS, TEAM_SIZE, team_of_seat};

use crate::faces::{Face, face};
use crate::hud::{
    Act, BUTTON, BUTTON_GOLD, DIM, FRAME, FRAME_SOFT, GOLD, Icons, PANEL, TEXT, Tint, Tip, WELL, framed,
    icon_image, initials, outline, prose, shade, text, title,
};
use crate::session::{Phase, Session};
use crate::skin::{self, plates};
use crate::view::{CHAMPION, TEAM};

/// Places on a team's bench (it holds at most what was never dealt).
const BENCH: usize = 10;
/// How long champion select runs, at most, for the bar (s): the server's default. A longer select
/// starts the bar full until it's within this.
const SELECT_SECS: f64 = 20.0;
/// The keys' letters over the abilities.
const KEYS: [&str; 4] = ["Q", "W", "E", "R"];

#[derive(Component)]
struct Screen;

/// What some words say.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Words {
    Seconds,
    TeamHead,
    /// A teammate's row (0 to 4, in seat order): their champion, its role and who plays it, the
    /// badge's initials.
    RowChamp(usize),
    RowWho(usize),
    RowInitials(usize),
    RowTag(usize),
    Role,
    Name,
    Title,
    AbilityName(usize),
    BenchHead,
    BenchName(usize),
    BenchRole(usize),
    BenchInitials(usize),
    Reroll,
    Ready,
}

/// The bar under the seconds.
#[derive(Component)]
struct TimeBar;

/// A teammate's row's frame (gold for the player's own) and its badge (the champion's colour).
#[derive(Component)]
struct RowFrame(usize);
#[derive(Component)]
struct RowBadge(usize);

/// The held champion's ability icons.
#[derive(Component)]
struct PickIcon(usize);

/// The colour band over the held champion's card.
#[derive(Component)]
struct PickBand;

/// A bench tile, shown while the bench reaches it, and its badge.
#[derive(Component)]
struct BenchTile(usize);
#[derive(Component)]
struct BenchBadge(usize);

/// The line said while the bench is empty.
#[derive(Component)]
struct BenchEmpty;

#[derive(Component)]
struct RerollButton;
#[derive(Component)]
struct ReadyButton;

/// A button's colours, to change together.
type ButtonLook = (&'static mut Tint, &'static mut BackgroundColor, &'static mut BorderColor);
/// The ready button's colours and the finish over them.
type ReadyLook = (ButtonLook, &'static mut MaterialNode<skin::Plate>);
/// The bench's tiles, not the line for an empty one or the tiles' badges.
type TileOnly = (Without<BenchEmpty>, Without<BenchBadge>);

pub struct SelectPlugin;

impl Plugin for SelectPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, setup).add_systems(
            Update,
            (show, words, bar, rows, held, bench, buttons).after(crate::net::NetSet::Send),
        );
    }
}

/// A champion's badge: its colour, its initials, a darker rim.
fn badge(
    commands: &mut Commands,
    side: f32,
    size: f32,
    words: Words,
    marker: impl Bundle,
    who: Face,
) -> Entity {
    commands
        .spawn((
            Node {
                width: px(side),
                height: px(side),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(3)),
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            BackgroundColor(WELL),
            MaterialNode(plates::FINISH_WIDE),
            marker,
            // The face over the initials, once its picture is in (`faces.rs`).
            children![(text("", size, TEXT), outline(), words), face(who)],
        ))
        .id()
}

/// A tall button with words on it.
fn big_button(
    commands: &mut Commands,
    act: Act,
    words: Words,
    width: f32,
    base: Color,
    marker: impl Bundle,
) -> Entity {
    commands
        .spawn((
            Button,
            Node {
                width: px(width),
                height: px(52),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BorderColor::all(FRAME),
            BackgroundColor(base),
            Tint(base),
            MaterialNode(plates::FINISH_WIDE),
            skin::drop_shadow(),
            act,
            marker,
            children![(title("", 17.0, TEXT), outline(), words)],
        ))
        .id()
}

#[allow(clippy::too_many_lines)]
fn setup(mut commands: Commands) {
    // The top: what this is, the seconds, the bar.
    let head = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(4),
            ..default()
        })
        .with_children(|h| {
            h.spawn((title("CHOOSE YOUR CHAMPION", 22.0, GOLD), outline()));
            h.spawn((
                title("", 46.0, Color::srgb(1.0, 0.96, 0.88)),
                TextShadow { offset: Vec2::new(0.0, 3.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
                Words::Seconds,
            ));
            h.spawn((
                Node {
                    width: px(460),
                    height: px(6),
                    border: UiRect::all(px(1)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.07)),
                children![(
                    Node { width: percent(100), height: percent(100), ..default() },
                    BackgroundColor(skin::EMBER_HI),
                    skin::halo(skin::EMBER, 0.8, 10.0),
                    TimeBar,
                )],
            ));
        })
        .id();

    // The team down the left.
    let mut team = vec![commands.spawn((title("", 15.0, TEXT), outline(), Words::TeamHead)).id()];
    for i in 0..TEAM_SIZE {
        let b = badge(&mut commands, 48.0, 17.0, Words::RowInitials(i), RowBadge(i), Face::SelectRow(i));
        let who = commands
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: px(2),
                ..default()
            })
            .with_children(|w| {
                w.spawn((text("", 17.0, TEXT), Words::RowChamp(i)));
                w.spawn((text("", 13.0, DIM), Words::RowWho(i)));
            })
            .id();
        let tag = commands.spawn((text("", 13.0, GOLD), Words::RowTag(i))).id();
        let row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    height: px(64),
                    padding: UiRect::axes(px(8), px(6)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BorderColor::all(FRAME_SOFT),
                BackgroundColor(PANEL),
                skin::finish(),
                RowFrame(i),
            ))
            .add_children(&[b, who, tag])
            .id();
        team.push(row);
    }
    let team = commands
        .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(7), width: px(310), ..default() })
        .add_children(&team)
        .id();

    // The champion held, in the middle: a band of its colour, its role, name and title, its kit.
    let abilities: Vec<Entity> = (0..4)
        .map(|k| {
            let key = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(-1),
                        bottom: px(-1),
                        padding: UiRect::axes(px(5), px(1)),
                        border_radius: BorderRadius::px(4.0, 0.0, 4.0, 0.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                    children![text(KEYS[k], 13.0, GOLD)],
                ))
                .id();
            let icon = commands
                .spawn((
                    Node {
                        width: px(76),
                        height: px(76),
                        border: UiRect::all(px(3)),
                        border_radius: BorderRadius::all(px(4)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    skin::plate(if k == 3 { plates::SLOT_READY } else { plates::SLOT_BRONZE }),
                    Interaction::None,
                    Tip::Pick(k),
                    children![(
                        Node { width: percent(100), height: percent(100), ..default() },
                        ImageNode::default(),
                        PickIcon(k),
                    )],
                ))
                .add_child(key)
                .id();
            let name = commands
                .spawn((
                    text("", 14.0, TEXT),
                    TextLayout::justify(Justify::Center),
                    Node { max_width: px(104), ..default() },
                    Words::AbilityName(k),
                ))
                .id();
            commands
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(6),
                    width: px(108),
                    ..default()
                })
                .add_children(&[icon, name])
                .id()
        })
        .collect();
    let kit = commands
        .spawn(Node { column_gap: px(12), margin: UiRect::top(px(10)), ..default() })
        .add_children(&abilities)
        .id();
    let band = commands
        .spawn((Node { width: percent(100), height: px(4), ..default() }, BackgroundColor(GOLD), PickBand))
        .id();
    // The champion's face, over its name.
    let portrait = commands
        .spawn((
            Node {
                width: px(112),
                height: px(112),
                margin: UiRect::top(px(12)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(4)),
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(FRAME),
            BackgroundColor(WELL),
            children![face(Face::Held)],
        ))
        .id();
    let role = commands
        .spawn((text("", 14.0, GOLD), Node { margin: UiRect::top(px(8)), ..default() }, Words::Role))
        .id();
    let name = commands
        .spawn((
            title("", 50.0, Color::srgb(1.0, 0.96, 0.88)),
            TextShadow { offset: Vec2::new(0.0, 3.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
            Words::Name,
        ))
        .id();
    let title = commands.spawn((prose("", 17.0, DIM), Words::Title)).id();
    let hint =
        commands.spawn(prose("Hover an ability to read it.", 13.0, Color::srgba(0.56, 0.59, 0.66, 0.7))).id();
    let card = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(4),
                width: px(560),
                padding: UiRect::new(px(26), px(26), px(20), px(20)),
                border: UiRect::all(px(3)),
                ..default()
            },
            framed(),
        ))
        .add_children(&[band, portrait, role, name, title, kit, hint])
        .id();

    let middle = commands
        .spawn(Node {
            column_gap: px(28),
            align_items: AlignItems::Start,
            margin: UiRect::top(px(18)),
            ..default()
        })
        .add_children(&[team, card])
        .id();
    // A spacer as wide as the team, so the card sits in the screen's middle.
    let spacer = commands.spawn(Node { width: px(310), ..default() }).id();
    commands.entity(middle).add_child(spacer);

    // The bench, under the card.
    let tiles: Vec<Entity> = (0..BENCH)
        .map(|k| {
            let b = badge(&mut commands, 32.0, 12.0, Words::BenchInitials(k), BenchBadge(k), Face::Bench(k));
            let words = commands
                .spawn(Node { flex_direction: FlexDirection::Column, ..default() })
                .with_children(|w| {
                    w.spawn((text("", 15.0, TEXT), Words::BenchName(k)));
                    w.spawn((text("", 12.0, DIM), Words::BenchRole(k)));
                })
                .id();
            commands
                .spawn((
                    Button,
                    Node {
                        width: px(150),
                        height: px(44),
                        align_items: AlignItems::Center,
                        column_gap: px(8),
                        padding: UiRect::horizontal(px(6)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(3)),
                        display: Display::None,
                        ..default()
                    },
                    BorderColor::all(FRAME_SOFT),
                    BackgroundColor(BUTTON),
                    Tint(BUTTON),
                    skin::finish(),
                    Act::Swap(k),
                    BenchTile(k),
                ))
                .add_children(&[b, words])
                .id()
        })
        .collect();
    let bench_head = commands.spawn((text("", 13.0, DIM), Words::BenchHead)).id();
    let empty = commands
        .spawn((
            prose("Empty. A reroll sends your champion here, for anyone on your team to take.", 14.0, DIM),
            Node { height: px(44), align_items: AlignItems::Center, ..default() },
            BenchEmpty,
        ))
        .id();
    let tiles = commands
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            justify_content: JustifyContent::Center,
            max_width: px(800),
            column_gap: px(6),
            row_gap: px(6),
            ..default()
        })
        .add_children(&tiles)
        .add_child(empty)
        .id();
    let bench = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(8),
            margin: UiRect::top(px(18)),
            ..default()
        })
        .add_children(&[bench_head, tiles])
        .id();

    let reroll = big_button(&mut commands, Act::RerollChampion, Words::Reroll, 230.0, BUTTON, RerollButton);
    let ready = big_button(&mut commands, Act::Ready, Words::Ready, 300.0, BUTTON_GOLD, ReadyButton);
    let actions = commands
        .spawn(Node { column_gap: px(14), margin: UiRect::top(px(16)), ..default() })
        .add_children(&[reroll, ready])
        .id();

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::top(percent(3)),
                display: Display::None,
                ..default()
            },
            crate::hud::down(Color::srgba(0.02, 0.03, 0.05, 0.80), Color::srgba(0.01, 0.015, 0.03, 0.94)),
            Interaction::None,
            // Over the HUD's corners (the minimap), under the tooltip.
            GlobalZIndex(5),
            Screen,
        ))
        .add_children(&[head, middle, bench, actions]);
}

fn show(session: Res<Session>, mut q: Query<&mut Node, With<Screen>>) {
    let want = if session.phase == Phase::Selecting && session.select.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut q {
        if node.display != want {
            node.display = want;
        }
    }
}

/// Champion `c`'s definition, if `c` is one.
fn def(c: u8) -> Option<&'static el_sim::content::ChampDef> {
    CHAMPIONS.get(usize::from(c))
}

/// The seats of the player's team in champion select, in seat order.
fn team_seats(seat: usize) -> impl Iterator<Item = usize> {
    let team = team_of_seat(seat);
    (0..SEATS).filter(move |&k| team_of_seat(k) == team)
}

fn words(session: Res<Session>, mut q: Query<(&Words, &mut Text)>) {
    let Some(s) = session.select.as_ref().filter(|_| session.phase == Phase::Selecting) else { return };
    let left = (s.ends_at - crate::net::now_s()).max(0.0).ceil() as u32;
    let mine = s.mine().and_then(def);
    let seat_of = |i: usize| team_seats(s.seat).nth(i);
    let pick_of = |k: usize| s.picks.get(k).copied().and_then(def);
    for (w, mut t) in &mut q {
        let want: String = match *w {
            Words::Seconds => left.to_string(),
            Words::TeamHead => {
                if team_of_seat(s.seat) == 0 {
                    "DAWN, YOUR TEAM".into()
                } else {
                    "DUSK, YOUR TEAM".into()
                }
            }
            Words::RowChamp(i) => seat_of(i).and_then(pick_of).map_or("...".into(), |d| d.name.into()),
            Words::RowWho(i) => seat_of(i).map_or(String::new(), |k| {
                let role = pick_of(k).map_or("", |d| d.role.name());
                let bot = session.roster.get(k).is_some_and(|p| p.bot);
                let who: String = session.name(k).chars().take(18).collect();
                let who = if bot { format!("{who} (bot)") } else { who };
                if role.is_empty() { who } else { format!("{role}, {who}") }
            }),
            Words::RowInitials(i) => seat_of(i)
                .and_then(|k| s.picks.get(k).copied())
                .filter(|&c| def(c).is_some())
                .map_or(String::new(), initials),
            Words::RowTag(i) => match seat_of(i) {
                Some(k) if k == s.seat && s.ready => "READY".into(),
                Some(k) if k == s.seat => "YOU".into(),
                _ => String::new(),
            },
            Words::Role => mine.map_or(String::new(), |d| d.role.name().to_uppercase()),
            Words::Name => mine.map_or(String::new(), |d| d.name.into()),
            Words::Title => mine.map_or(String::new(), |d| d.title.into()),
            Words::AbilityName(k) => s.mine().map_or(String::new(), |c| ability(c, k as u8).name.into()),
            Words::BenchHead => {
                if s.bench.is_empty() {
                    "THE BENCH".into()
                } else {
                    "THE BENCH: click a champion to take it, yours goes in its place".into()
                }
            }
            Words::BenchName(k) => {
                s.bench.get(k).copied().and_then(def).map_or(String::new(), |d| d.name.into())
            }
            Words::BenchRole(k) => {
                s.bench.get(k).copied().and_then(def).map_or(String::new(), |d| d.role.name().into())
            }
            Words::BenchInitials(k) => {
                s.bench.get(k).copied().filter(|&c| def(c).is_some()).map_or(String::new(), initials)
            }
            Words::Reroll => match s.rerolls {
                0 => "NO REROLLS LEFT".into(),
                n => format!("REROLL  ({n})"),
            },
            Words::Ready => {
                if s.ready {
                    "READY: WAITING FOR THE REST".into()
                } else {
                    "READY".into()
                }
            }
        };
        if t.as_str() != want {
            **t = want;
        }
    }
}

fn bar(session: Res<Session>, mut q: Query<&mut Node, With<TimeBar>>) {
    let Some(s) = session.select.as_ref().filter(|_| session.phase == Phase::Selecting) else { return };
    let left = (s.ends_at - crate::net::now_s()).max(0.0);
    let share = (left / SELECT_SECS).clamp(0.0, 1.0) as f32;
    // A step of a fifth of a percent: the bar is moved only when it shows.
    let want = percent((share * 500.0).round() / 5.0);
    for mut node in &mut q {
        if node.width != want {
            node.width = want;
        }
    }
}

/// Each teammate's row: its frame gold for the player's own, its badge its champion's colour.
fn rows(
    session: Res<Session>,
    mut frames: Query<(&RowFrame, &mut BorderColor, &mut BackgroundColor), Without<RowBadge>>,
    mut badges: Query<(&RowBadge, &mut BackgroundColor), Without<RowFrame>>,
) {
    let Some(s) = session.select.as_ref().filter(|_| session.phase == Phase::Selecting) else { return };
    let team = TEAM[usize::from(team_of_seat(s.seat))];
    for (f, mut border, mut bg) in &mut frames {
        let own = team_seats(s.seat).nth(f.0) == Some(s.seat);
        let (b, back) = if own { (GOLD, shade(PANEL, 0.05)) } else { (team.with_alpha(0.35), PANEL) };
        border.set_if_neq(BorderColor::all(b));
        bg.set_if_neq(BackgroundColor(back));
    }
    for (b, mut bg) in &mut badges {
        let c = team_seats(s.seat).nth(b.0).and_then(|k| s.picks.get(k).copied());
        let colour =
            c.filter(|&c| def(c).is_some()).map_or(WELL, |c| CHAMPION[usize::from(c) % CHAMPION.len()]);
        bg.set_if_neq(BackgroundColor(colour));
    }
}

/// The held champion's kit's icons, and the band over its card in its colour.
fn held(
    session: Res<Session>,
    mut icons: ResMut<Icons>,
    mut images: ResMut<Assets<Image>>,
    mut q: Query<(&PickIcon, &mut ImageNode)>,
    mut band: Query<&mut BackgroundColor, With<PickBand>>,
) {
    let Some(c) =
        session.select.as_ref().filter(|_| session.phase == Phase::Selecting).and_then(|s| s.mine())
    else {
        return;
    };
    for (icon, mut node) in &mut q {
        let h = icons
            .0
            .entry((c, icon.0 as u8))
            .or_insert_with(|| images.add(icon_image(el_client_core::icons::ability_icon(c, icon.0 as u8))))
            .clone();
        if node.image != h {
            node.image = h;
        }
    }
    for mut bg in &mut band {
        bg.set_if_neq(BackgroundColor(CHAMPION[usize::from(c) % CHAMPION.len()]));
    }
}

/// As many bench tiles as the bench holds, each its champion's colour; the line for an empty one.
fn bench(
    session: Res<Session>,
    mut tiles: Query<(&BenchTile, &mut Node), TileOnly>,
    mut badges: Query<(&BenchBadge, &mut BackgroundColor)>,
    mut empty: Query<&mut Node, (With<BenchEmpty>, Without<BenchTile>)>,
) {
    let Some(s) = session.select.as_ref().filter(|_| session.phase == Phase::Selecting) else { return };
    for (t, mut node) in &mut tiles {
        let want = if t.0 < s.bench.len() { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    for (b, mut bg) in &mut badges {
        if let Some(&c) = s.bench.get(b.0).filter(|&&c| def(c).is_some()) {
            bg.set_if_neq(BackgroundColor(CHAMPION[usize::from(c) % CHAMPION.len()]));
        }
    }
    let want = if s.bench.is_empty() { Display::Flex } else { Display::None };
    for mut node in &mut empty {
        if node.display != want {
            node.display = want;
        }
    }
}

/// The reroll button greyed once there are none left; ready green once said.
fn buttons(
    session: Res<Session>,
    mut reroll: Query<ButtonLook, (With<RerollButton>, Without<ReadyButton>)>,
    mut ready: Query<ReadyLook, (With<ReadyButton>, Without<RerollButton>)>,
) {
    let Some(s) = session.select.as_ref().filter(|_| session.phase == Phase::Selecting) else { return };
    let (base, border) = if s.rerolls > 0 {
        (BUTTON, FRAME)
    } else {
        (Color::srgb(0.07, 0.075, 0.09), Color::srgb(0.22, 0.22, 0.25))
    };
    for (mut tint, mut bg, mut b) in &mut reroll {
        if tint.0 != base {
            tint.0 = base;
            bg.0 = base;
            *b = BorderColor::all(border);
        }
    }
    let (base, border) = if s.ready {
        (Color::srgb(0.10, 0.24, 0.14), Color::srgb(0.36, 0.78, 0.44))
    } else {
        (BUTTON_GOLD, skin::EMBER_HI)
    };
    // Ready to be pressed: lit from inside, a sheen crossing it; said: steady.
    let finish = if s.ready { plates::FINISH_WIDE } else { plates::FINISH_HOT };
    for ((mut tint, mut bg, mut b), mut plate) in &mut ready {
        if tint.0 != base {
            tint.0 = base;
            bg.0 = base;
            *b = BorderColor::all(border);
        }
        if plate.0 != finish {
            plate.0 = finish.clone();
        }
    }
}

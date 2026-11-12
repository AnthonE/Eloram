//! The shop (P), laid out as League's: the items as a grid of framed icons with their prices on
//! the left (what's recommended for the champion's role first, then the parts and the finished
//! ones apart), the one picked on the right with what it gives and the button that buys it, and
//! the champion's own six along the foot, each with what selling it gives back.
//!
//! A click picks an item (the pane shows what's under the pointer while it's over one); a second
//! click on it, a right click, or BUY buys it. A tile's frame says its state: ember with a light
//! running round it picked, bronze under the pointer, iron and the icon dimmed while the gold's
//! short.

use bevy::prelude::*;
use el_client_core::tips;
use el_proto::Order;
use el_sim::content::champions::CHAMPIONS;
use el_sim::content::items::{self, ITEMS};

use crate::hud::{
    Act, DIM, GOLD, ItemIcon, Panel, SHORT, TEXT, Tint, Tip, framed, outline, prose, sunk, text,
};
use crate::input::Controls;
use crate::session::Session;
use crate::skin::{self, Plate, Press, plates};

/// The lines of the pane: what an item gives, most of them.
const LINES: usize = 5;
/// A second click this soon on the same tile buys (s).
const DOUBLE: f64 = 0.45;
const TILE: f32 = 56.0;
/// An item under this is a part.
const PART_UNDER: u32 = 1_000;

/// The item picked, and the last tile clicked (for a double click).
#[derive(Resource, Default)]
struct Picked {
    item: Option<usize>,
    last: Option<(usize, f64)>,
}

/// A tile: the item it sells (the Recommended row's change with the champion: `rec`), and its
/// parts.
#[derive(Component)]
struct Tile {
    item: usize,
    /// Which of the Recommended row's four it is.
    rec: Option<usize>,
    icon: Entity,
    frame: Entity,
    cost: Entity,
}

/// A recommended item's mark, on its tile in the grid.
#[derive(Component)]
struct Mark(usize);

/// The pane's words.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Words {
    Rec,
    Gold,
    Name,
    Kind,
    Cost,
    /// A line of what it gives: its number, and what of.
    Value(usize),
    What(usize),
    Active,
    /// Why it can't be bought, if it can't.
    Why,
    Buy,
    /// What selling the item in a slot gives back.
    Sell(usize),
}

/// The pane's picture.
#[derive(Component)]
struct Picture;

/// The button that buys what's picked.
#[derive(Component)]
struct Buy;

pub struct ShopPlugin;

impl Plugin for ShopPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picked>().add_systems(PostStartup, setup).add_systems(
            Update,
            (
                clicks.after(crate::net::NetSet::Receive).before(crate::net::NetSet::Send),
                (recommended, tiles, pane).chain().after(crate::net::NetSet::Send),
            ),
        );
    }
}

/// A tile: the icon in its frame, the price under it.
fn tile(commands: &mut Commands, item: usize, rec: Option<usize>) -> Entity {
    let icon = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(2),
                top: px(2),
                width: px(TILE - 4.0),
                height: px(TILE - 4.0),
                ..default()
            },
            ImageNode::default(),
            Visibility::Hidden,
            ItemIcon::Shop(item),
        ))
        .id();
    // The frame is a plate, clear inside its rim, over the picture.
    let frame = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                border: UiRect::all(px(3)),
                ..default()
            },
            skin::plate(plates::TILE),
        ))
        .id();
    let mark = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(-4),
                right: px(-4),
                width: px(9),
                height: px(9),
                border: UiRect::all(px(1)),
                ..default()
            },
            UiTransform { rotation: Rot2::degrees(45.0), ..UiTransform::IDENTITY },
            BackgroundColor(GOLD),
            BorderColor::all(Color::srgb(0.30, 0.20, 0.04)),
            if rec.is_some() { Visibility::Inherited } else { Visibility::Hidden },
            Mark(item),
        ))
        .id();
    let well = commands
        .spawn((
            Node { width: px(TILE), height: px(TILE), ..default() },
            BackgroundColor(Color::srgb(0.02, 0.024, 0.036)),
        ))
        .add_children(&[icon, frame, mark])
        .id();
    let cost = commands.spawn((text("", 13.0, GOLD), outline())).id();
    commands
        .spawn((
            Button,
            Node {
                width: px(TILE + 8.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(2),
                padding: UiRect::axes(px(4), px(3)),
                ..default()
            },
            Tile { item, rec, icon, frame, cost },
        ))
        .add_children(&[well, cost])
        .id()
}

/// A titled run of tiles.
fn section(commands: &mut Commands, title: Entity, tiles: &[Entity]) -> Entity {
    let row = commands
        .spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(4), row_gap: px(4), ..default() })
        .add_children(tiles)
        .id();
    commands
        .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(5), ..default() })
        .add_children(&[title, row])
        .id()
}

/// A coin: the gold's mark beside its number.
fn coin(side: f32) -> impl Bundle {
    (
        Node {
            width: px(side),
            height: px(side),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(Color::srgb(0.95, 0.72, 0.22)),
        BorderColor {
            top: Color::srgb(1.0, 0.92, 0.60),
            left: Color::srgb(1.0, 0.92, 0.60),
            bottom: Color::srgb(0.52, 0.33, 0.06),
            right: Color::srgb(0.52, 0.33, 0.06),
        },
    )
}

#[allow(clippy::too_many_lines)]
fn setup(mut commands: Commands) {
    let commands = &mut commands;
    // The head: the title, and the purse.
    let title = commands.spawn((skin::display("SHOP", 26.0, GOLD), outline())).id();
    let purse = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(8),
                padding: UiRect::axes(px(14), px(5)),
                border: UiRect::all(px(1)),
                ..default()
            },
            skin::plate(plates::INSET),
            children![coin(18.0), (text("", 20.0, GOLD), outline(), Words::Gold)],
        ))
        .id();
    let head = commands
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .add_children(&[title, purse])
        .id();

    // The grid.
    let rec: Vec<Entity> = (0..4).map(|j| tile(commands, 0, Some(j))).collect();
    let (parts, done): (Vec<usize>, Vec<usize>) = (0..ITEMS.len()).partition(|&k| ITEMS[k].cost < PART_UNDER);
    let parts: Vec<Entity> = parts.into_iter().map(|k| tile(commands, k, None)).collect();
    let done: Vec<Entity> = done.into_iter().map(|k| tile(commands, k, None)).collect();
    let rec_title = commands.spawn((text("", 12.0, GOLD), Words::Rec)).id();
    let parts_title = commands.spawn(text("COMPONENTS", 12.0, DIM)).id();
    let done_title = commands.spawn(text("FINISHED ITEMS", 12.0, DIM)).id();
    let rec = section(commands, rec_title, &rec);
    let parts = section(commands, parts_title, &parts);
    let done = section(commands, done_title, &done);
    let rule_a = commands.spawn(skin::rule(true)).id();
    let rule_b = commands.spawn(skin::rule(true)).id();
    let grid = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            width: px(10.0 * (TILE + 12.0)),
            flex_shrink: 0.0,
            ..default()
        })
        .add_children(&[rec, rule_a, parts, rule_b, done])
        .id();

    // The pane: the picture and the name, what it gives, the button.
    let picture = commands
        .spawn((
            Node { width: px(84), height: px(84), flex_shrink: 0.0, ..default() },
            BackgroundColor(Color::srgb(0.02, 0.024, 0.036)),
            children![
                (
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(3),
                        top: px(3),
                        width: px(78),
                        height: px(78),
                        ..default()
                    },
                    ImageNode::default(),
                    Visibility::Hidden,
                    ItemIcon::Shop(0),
                    Picture,
                ),
                (
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        border: UiRect::all(px(4)),
                        ..default()
                    },
                    skin::plate(plates::GRAND),
                ),
            ],
        ))
        .id();
    let name = commands
        .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(3), flex_shrink: 1.0, ..default() })
        .with_children(|p| {
            p.spawn((skin::display("", 19.0, GOLD), outline(), Words::Name));
            p.spawn((text("", 11.0, DIM), Words::Kind));
            p.spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    margin: UiRect::top(px(4)),
                    ..default()
                },
                children![coin(13.0), (text("", 17.0, GOLD), outline(), Words::Cost)],
            ));
        })
        .id();
    let top = commands
        .spawn(Node { column_gap: px(12), align_items: AlignItems::Center, ..default() })
        .add_children(&[picture, name])
        .id();
    let rule_c = commands.spawn(skin::rule(false)).id();
    let lines = commands
        .spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(4), ..default() })
        .with_children(|p| {
            for i in 0..LINES {
                p.spawn(Node { column_gap: px(7), align_items: AlignItems::Baseline, ..default() })
                    .with_children(|l| {
                        l.spawn((text("", 16.0, GOLD), Words::Value(i)));
                        l.spawn((prose("", 15.0, TEXT), Words::What(i)));
                    });
            }
        })
        .id();
    let active = commands.spawn((prose("", 14.0, Color::srgb(0.74, 0.80, 0.90)), Words::Active)).id();
    let gap = commands.spawn(Node { flex_grow: 1.0, ..default() }).id();
    let why = commands.spawn((text("", 13.0, SHORT), TextLayout::justify(Justify::Center), Words::Why)).id();
    let buy = commands
        .spawn((
            Button,
            Node {
                height: px(44),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(3)),
                ..default()
            },
            skin::plate(plates::GO),
            Press::GO,
            Buy,
            children![(skin::display("", 18.0, Color::srgb(1.0, 0.95, 0.82)), outline(), Words::Buy)],
        ))
        .id();
    let pane = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                flex_grow: 1.0,
                padding: UiRect::all(px(16)),
                border: UiRect::all(px(1)),
                ..default()
            },
            skin::plate(plates::INSET),
        ))
        .add_children(&[top, rule_c, lines, active, gap, why, buy])
        .id();
    let body = commands.spawn(Node { column_gap: px(16), ..default() }).add_children(&[grid, pane]).id();

    // The foot: the champion's own, and what a click does.
    let bags: Vec<Entity> = (0..items::SLOTS)
        .map(|k| {
            let slot = commands
                .spawn((
                    Button,
                    Node {
                        width: px(44),
                        height: px(44),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(3)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    sunk(),
                    Tint(Color::srgb(0.030, 0.036, 0.054)),
                    Act::Sell(k as u8),
                    Tip::Item(k),
                    children![(
                        Node {
                            position_type: PositionType::Absolute,
                            width: percent(100),
                            height: percent(100),
                            ..default()
                        },
                        ImageNode::default(),
                        Visibility::Hidden,
                        ItemIcon::Held(k),
                    )],
                ))
                .id();
            let back = commands.spawn((text("", 11.0, DIM), Words::Sell(k))).id();
            commands
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(1),
                    min_height: px(60),
                    ..default()
                })
                .add_children(&[slot, back])
                .id()
        })
        .collect();
    let bags_title = commands.spawn(text("YOUR ITEMS", 12.0, DIM)).id();
    let bags = commands.spawn(Node { column_gap: px(6), ..default() }).add_children(&bags).id();
    let held = commands
        .spawn(Node { column_gap: px(14), align_items: AlignItems::Center, ..default() })
        .add_children(&[bags_title, bags])
        .id();
    let hint = commands
        .spawn((
            prose(
                "Click an item to look at it; click it again, or right click, to buy.\nClick one of yours to sell it back. P closes.",
                12.0,
                DIM,
            ),
            TextLayout::justify(Justify::Right),
        ))
        .id();
    let foot = commands
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .add_children(&[held, hint])
        .id();
    let rule_head = commands.spawn(skin::rule(false)).id();
    let rule_foot = commands.spawn(skin::rule(true)).id();
    let panel = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                width: px(1040),
                padding: UiRect::axes(px(22), px(16)),
                border: UiRect::all(px(3)),
                ..default()
            },
            framed(),
        ))
        .add_children(&[head, rule_head, body, rule_foot, foot])
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: percent(7),
                width: percent(100),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            Panel::Shop,
        ))
        .add_child(panel);
}

/// Whether the champion can buy `item` now, and if not, why.
fn refused(ch: &el_sim::game::Champ, item: usize) -> Option<String> {
    let cost = ITEMS[item].cost as f32;
    if ch.items.iter().all(|&i| i != items::NONE) {
        Some("Your six slots are full: sell one first.".into())
    } else if ch.gold < cost {
        Some(format!("{:.0} more gold", (cost - ch.gold).ceil()))
    } else {
        None
    }
}

/// The clicks: a tile picked, bought on the second or with the right button; BUY.
fn clicks(
    session: Res<Session>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut controls: ResMut<Controls>,
    mut picked: ResMut<Picked>,
    tiles: Query<(&Interaction, Ref<Interaction>, &Tile)>,
    buy: Query<&Interaction, (With<Buy>, Changed<Interaction>)>,
) {
    if !controls.shop_open || session.client.is_none() {
        return;
    }
    let now = crate::net::now_s();
    for (i, changed, t) in &tiles {
        match *i {
            Interaction::Pressed if changed.is_changed() => {
                let again = picked.last.is_some_and(|(k, at)| k == t.item && now - at < DOUBLE);
                if again {
                    controls.pending.push(Order::Buy { item: t.item as u8 });
                    picked.last = None;
                } else {
                    picked.last = Some((t.item, now));
                }
                picked.item = Some(t.item);
            }
            Interaction::Hovered if mouse.just_pressed(MouseButton::Right) => {
                picked.item = Some(t.item);
                controls.pending.push(Order::Buy { item: t.item as u8 });
            }
            _ => {}
        }
    }
    if buy.iter().any(|i| *i == Interaction::Pressed)
        && let Some(item) = picked.item
    {
        controls.pending.push(Order::Buy { item: item as u8 });
    }
}

/// The Recommended row's items, for the champion's role, the marks on the grid's tiles for them,
/// and the first of them picked until the player picks one.
fn recommended(
    session: Res<Session>,
    mut picked: ResMut<Picked>,
    mut tiles: Query<&mut Tile>,
    mut icons: Query<&mut ItemIcon>,
    mut marks: Query<(&Mark, &mut Visibility)>,
) {
    let Some(c) = session.client.as_deref() else {
        // A new match starts with nothing picked.
        if picked.item.is_some() {
            *picked = Picked::default();
        }
        return;
    };
    let def = usize::from(c.pred.champs[c.seat].def) % CHAMPIONS.len();
    let rec = tips::recommended(CHAMPIONS[def].role);
    for mut t in &mut tiles {
        let Some(j) = t.rec else { continue };
        let k = usize::from(rec[j]);
        if t.item != k {
            t.item = k;
            if let Ok(mut i) = icons.get_mut(t.icon) {
                *i = ItemIcon::Shop(k);
            }
        }
    }
    if picked.item.is_none() {
        picked.item = Some(usize::from(rec[0]));
    }
    for (m, mut v) in &mut marks {
        let on = rec.contains(&(m.0 as u8));
        v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
}

/// Each tile's frame, picture and price, by its state.
fn tiles(
    session: Res<Session>,
    controls: Res<Controls>,
    picked: Res<Picked>,
    q: Query<(&Tile, &Interaction)>,
    mut frames: Query<&mut MaterialNode<Plate>>,
    mut pictures: Query<&mut ImageNode>,
    mut costs: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(c) = session.client.as_deref().filter(|_| controls.shop_open) else { return };
    let ch = &c.pred.champs[c.seat];
    for (t, i) in &q {
        let affords = ch.gold >= ITEMS[t.item].cost as f32;
        let want = if picked.item == Some(t.item) {
            &plates::TILE_PICKED
        } else if *i != Interaction::None {
            &plates::TILE_OVER
        } else if affords {
            &plates::TILE
        } else {
            &plates::TILE_SHORT
        };
        if let Ok(mut f) = frames.get_mut(t.frame)
            && f.0 != *want
        {
            f.0 = want.clone();
        }
        let tint = if affords { Color::WHITE } else { Color::srgb(0.42, 0.42, 0.46) };
        if let Ok(mut p) = pictures.get_mut(t.icon)
            && p.color != tint
        {
            p.color = tint;
        }
        if let Ok((mut words, mut color)) = costs.get_mut(t.cost) {
            let said = ITEMS[t.item].cost.to_string();
            if words.as_str() != said {
                **words = said;
            }
            color.set_if_neq(TextColor(if affords { GOLD } else { SHORT }));
        }
    }
}

/// The pane: the item under the pointer, or the one picked.
#[allow(clippy::type_complexity)]
fn pane(
    session: Res<Session>,
    controls: Res<Controls>,
    picked: Res<Picked>,
    over: Query<(&Tile, &Interaction)>,
    mut words: Query<(&Words, &mut Text)>,
    mut picture: Query<&mut ItemIcon, With<Picture>>,
    mut buy: Query<(&mut Press, &mut MaterialNode<Plate>, &Interaction), With<Buy>>,
) {
    let Some(c) = session.client.as_deref().filter(|_| controls.shop_open) else { return };
    let ch = &c.pred.champs[c.seat];
    let shown = over.iter().find(|(_, i)| **i != Interaction::None).map(|(t, _)| t.item).or(picked.item);
    let Some(item) = shown.filter(|&k| k < ITEMS.len()) else { return };
    let it = &ITEMS[item];
    for mut p in &mut picture {
        if *p != ItemIcon::Shop(item) {
            *p = ItemIcon::Shop(item);
        }
    }
    let lines = tips::mods_lines(&it.mods);
    // BUY is for the one picked, whatever the pointer's over.
    let buying = picked.item.filter(|&k| k < ITEMS.len());
    let why = buying.and_then(|k| refused(ch, k));
    for (w, mut t) in &mut words {
        let said = match *w {
            Words::Rec => {
                let def = usize::from(ch.def) % CHAMPIONS.len();
                format!("RECOMMENDED FOR {}", CHAMPIONS[def].role.name().to_uppercase())
            }
            Words::Gold => format!("{:.0}", ch.gold.floor()),
            Words::Name => it.name.to_uppercase(),
            Words::Kind => {
                let kind = if it.cost < PART_UNDER { "COMPONENT" } else { "FINISHED ITEM" };
                let held = ch.items.iter().filter(|&&i| usize::from(i) == item).count();
                if held > 0 { format!("{kind}  ·  YOU HOLD {held}") } else { kind.into() }
            }
            Words::Cost => it.cost.to_string(),
            Words::Value(i) => {
                lines.get(i).and_then(|l| l.split_once(' ')).map_or(String::new(), |(v, _)| v.into())
            }
            Words::What(i) => {
                lines.get(i).and_then(|l| l.split_once(' ')).map_or(String::new(), |(_, w)| w.into())
            }
            Words::Active => {
                it.active.map_or(String::new(), |a| format!("Active: {}", tips::active_text(&a)))
            }
            Words::Why => why.clone().unwrap_or_default(),
            Words::Buy => buying.map_or(String::new(), |k| format!("BUY  ·  {}", ITEMS[k].cost)),
            Words::Sell(k) => match ch.items[k] {
                items::NONE => String::new(),
                held => format!("+{:.0}", (ITEMS[usize::from(held)].cost as f32 * items::SELL_SHARE).floor()),
            },
        };
        if t.as_str() != said {
            **t = said;
        }
    }
    for (mut press, mut plate, i) in &mut buy {
        let (want, rest) =
            if why.is_some() { (Press::WAIT, &plates::GO_WAIT) } else { (Press::GO, &plates::GO) };
        if press.idle != want.idle {
            *press = want;
            if *i == Interaction::None {
                plate.0 = rest.clone();
            }
        }
    }
}

//! Dying, as League shows it: the screen greyed, the time to respawn over the portrait (`hud.rs`),
//! and the death recap, a panel over the bottom's middle that says who killed the champion and what
//! hurt it in the seconds before, each line's damage split physical, magic and true
//! (`el_client_core::recap`).

use bevy::prelude::*;
use el_client_core::recap::{Hit, LINES, Line, Recap};
use el_proto::event::WireEvent;
use el_sim::content::augments::AUGMENTS;
use el_sim::content::champions::{CHAMPIONS, ability};
use el_sim::missile::Source;
use el_sim::units::{Kind, SEATS};

use crate::hud::{DIM, GOLD, TEXT, framed, outline, text};
use crate::session::Session;

/// Physical, magic and true damage's colours, as League's.
const SPLIT: [Color; 3] =
    [Color::srgb(0.96, 0.52, 0.28), Color::srgb(0.42, 0.62, 1.0), Color::srgb(0.95, 0.95, 0.97)];

#[derive(Component)]
struct Shade;

#[derive(Component)]
struct Panel;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Words {
    Title,
    Total,
    Name(usize),
    Amount(usize),
}

#[derive(Component)]
struct Row(usize);

/// A row's share of one damage type, as a stretch of its bar.
#[derive(Component)]
struct Split(usize, usize);

/// What the recap knows: the feed read so far, the hits, and what killed the champion last.
#[derive(Resource, Default)]
struct Told {
    read: u64,
    recap: Recap,
    fell: Option<Fell>,
}

struct Fell {
    killer: u8,
    lines: Vec<Line>,
    all: u32,
}

pub struct DeathPlugin;

impl Plugin for DeathPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Told>()
            .add_systems(PostStartup, setup)
            .add_systems(Update, (read, show).chain().after(crate::net::NetSet::Send));
    }
}

fn setup(mut commands: Commands) {
    // The grey over the world, under the HUD.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::srgba(0.16, 0.17, 0.19, 0.55)),
        GlobalZIndex(-10),
        Visibility::Hidden,
        Shade,
    ));
    let rows: Vec<Entity> = (0..LINES)
        .map(|i| {
            let bar = commands
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(4),
                        border_radius: BorderRadius::all(px(2)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.06)),
                ))
                .with_children(|b| {
                    for (k, c) in SPLIT.iter().enumerate() {
                        b.spawn((
                            Node { width: percent(0), height: percent(100), ..default() },
                            BackgroundColor(*c),
                            Split(i, k),
                        ));
                    }
                })
                .id();
            let head = commands
                .spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    width: percent(100),
                    ..default()
                })
                .with_children(|h| {
                    h.spawn((text("", 12.0, TEXT), Words::Name(i)));
                    h.spawn((text("", 12.0, GOLD), Words::Amount(i)));
                })
                .id();
            commands
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(3),
                        width: percent(100),
                        ..default()
                    },
                    Row(i),
                ))
                .add_children(&[head, bar])
                .id()
        })
        .collect();
    let title = commands.spawn((text("", 16.0, Color::srgb(0.95, 0.42, 0.38)), outline(), Words::Title)).id();
    let total = commands.spawn((text("", 11.0, DIM), Words::Total)).id();
    let body = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(7),
                width: px(320),
                padding: UiRect::axes(px(14), px(10)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            framed(),
        ))
        .add_children(&[title, total])
        .add_children(&rows)
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(200),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            Panel,
        ))
        .add_child(body);
}

/// The feed's hits on the player's champion, and its fall.
fn read(session: Res<Session>, mut told: ResMut<Told>) {
    let Some(c) = session.client.as_deref().filter(|_| session.watch.is_none()) else {
        return;
    };
    let told = &mut *told;
    if c.feed_n < told.read {
        // Another match.
        *told = Told::default();
    }
    let feed = el_client_core::client::FEED as u64;
    for n in told.read.max(c.feed_n.saturating_sub(feed))..c.feed_n {
        let (tick, e) = c.feed[(n % feed) as usize];
        match e {
            WireEvent::Damage { target, source, amount, kind, from } if usize::from(target) == c.seat => {
                told.recap.hit(Hit { tick, source, amount, kind, from });
            }
            WireEvent::Death { unit, killer } if usize::from(unit) == c.seat => {
                let (lines, all) = told.recap.sum(tick);
                told.fell = Some(Fell { killer, lines, all });
                told.recap.clear();
            }
            _ => {}
        }
    }
    told.read = c.feed_n;
}

/// Who a unit is, for the recap.
fn who(c: &el_client_core::Client, unit: u8) -> String {
    let u = usize::from(unit);
    if u < SEATS {
        return CHAMPIONS[usize::from(c.world.champs[u].def) % CHAMPIONS.len()].name.into();
    }
    match c.world.units.get(u).map(|u| u.kind) {
        Some(Kind::Turret) => "Turret".into(),
        Some(Kind::Nexus) => "Nexus".into(),
        Some(Kind::Inhibitor) => "Inhibitor".into(),
        Some(Kind::Fountain) => "Platform".into(),
        Some(_) => "Minions".into(),
        None => "Unknown".into(),
    }
}

/// What a hit was dealt with.
fn with(from: Source) -> String {
    match from {
        Source::Auto => "Attacks".into(),
        Source::Turret => "Turret shots".into(),
        Source::Fountain => "The platform's laser".into(),
        Source::Ability { champ, slot } => ability(champ, slot).name.into(),
        Source::Relic => "A relic".into(),
        Source::Augment(a) => AUGMENTS.get(usize::from(a)).map_or("An augment", |d| d.name).into(),
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn show(
    session: Res<Session>,
    told: Res<Told>,
    mut shade: Query<&mut Visibility, (With<Shade>, Without<Panel>)>,
    mut panel: Query<&mut Visibility, (With<Panel>, Without<Shade>)>,
    mut words: Query<(&Words, &mut Text)>,
    mut rows: Query<(&Row, &mut Node), Without<Split>>,
    mut splits: Query<(&Split, &mut Node), Without<Row>>,
    mut dev: ResMut<crate::dev_hooks::DevStatus>,
) {
    let c = session.client.as_deref().filter(|_| session.watch.is_none());
    let dead = c.is_some_and(|c| c.started() && !c.pred.units[c.seat].alive && c.world.outcome.is_none());
    let vis = if dead { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut shade {
        v.set_if_neq(vis);
    }
    let fell = told.fell.as_ref().filter(|_| dead);
    dev.set("death_screen", dead);
    dev.set("recap_lines", fell.map_or(0, |f| f.lines.len()));
    for mut v in &mut panel {
        v.set_if_neq(if fell.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    }
    let (Some(c), Some(f)) = (c, fell) else { return };
    let most = f.lines.first().map_or(1, Line::total).max(1);
    for (w, mut t) in &mut words {
        let s = match *w {
            Words::Title => format!("Slain by {}", who(c, f.killer)),
            Words::Total => {
                format!("{} damage taken in the last {} s", f.all, el_client_core::recap::WINDOW_SECS)
            }
            Words::Name(i) => f.lines.get(i).map_or(String::new(), |l| {
                let by = who(c, l.source);
                let w = with(l.from);
                if l.hits > 1 { format!("{w} ({by}) x{}", l.hits) } else { format!("{w} ({by})") }
            }),
            Words::Amount(i) => f.lines.get(i).map_or(String::new(), |l| l.total().to_string()),
        };
        if **t != s {
            **t = s;
        }
    }
    for (r, mut node) in &mut rows {
        let d = if r.0 < f.lines.len() { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
    }
    for (s, mut node) in &mut splits {
        let share = f.lines.get(s.0).map_or(0.0, |l| l.split[s.1] as f32 / most as f32 * 100.0);
        let w = percent(share);
        if node.width != w {
            node.width = w;
        }
    }
}

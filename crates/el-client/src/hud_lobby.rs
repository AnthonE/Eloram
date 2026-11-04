//! The screens around a match: a card in the middle while the page connects, signs in, waits in
//! the lobby or the queue (how long it's waited, who else is, when the bots take the empty seats,
//! and a button in or out of the queue), reconnects or has failed (and a button to load the page
//! again); and once a match is over, the result over the scoreboard (`hud.rs` keeps it up), the
//! score and the clock it ended on, the player's own line, and a button to queue again.

use bevy::prelude::*;
use el_sim::units::{SEATS, team_of_seat};

use crate::hud::{Act, BUTTON, BUTTON_GOLD, DIM, FRAME, GOLD, TEXT, TRIM, Tint, framed, mmss, outline, text};
use crate::session::{Phase, Session};

/// The colour of a defeat's title.
const LOSS: Color = Color::srgb(0.86, 0.36, 0.32);

#[derive(Component)]
struct Card;

#[derive(Component)]
struct Over;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Words {
    Title,
    Big,
    Line,
    Button,
    Result,
    Score,
    Mine,
}

/// The card's button: what it does is the phase's.
#[derive(Component)]
struct CardButton;

/// The sweep across the card while it waits.
#[derive(Component)]
struct Sweep;

/// The ribbon under the result, in its colour.
#[derive(Component)]
struct Ribbon;

/// When the queue was joined (s, the page's clock), for how long it's been.
#[derive(Resource, Default)]
struct Since {
    phase: Phase,
    at: f64,
}

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Since>()
            .add_systems(PostStartup, setup)
            .add_systems(Update, (show, words, sweep, card_button).after(crate::net::NetSet::Send));
    }
}

fn setup(mut commands: Commands) {
    // The card.
    let sweep = commands
        .spawn((
            Node {
                width: px(300),
                height: px(3),
                border_radius: BorderRadius::all(px(2)),
                overflow: Overflow::clip(),
                margin: UiRect::vertical(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
            children![(
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(30),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(GOLD),
                Sweep,
            )],
        ))
        .id();
    let button = commands
        .spawn((
            Button,
            Node {
                width: px(220),
                height: px(40),
                margin: UiRect::top(px(6)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BorderColor::all(FRAME),
            BackgroundColor(BUTTON),
            Tint(BUTTON),
            Act::Queue,
            CardButton,
            children![(text("", 14.0, TEXT), outline(), Words::Button)],
        ))
        .id();
    let body = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(6),
                min_width: px(440),
                max_width: px(560),
                padding: UiRect::new(px(28), px(28), px(22), px(24)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            framed(),
            children![
                text("ELORAM", 12.0, DIM),
                (text("", 22.0, GOLD), outline(), Words::Title),
                (text("", 40.0, TEXT), outline(), Words::Big),
                (text("", 13.0, DIM), TextLayout::justify(Justify::Center), Words::Line),
            ],
        ))
        .add_children(&[sweep, button])
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
            GlobalZIndex(4),
            Card,
        ))
        .add_child(body);

    // A match over: the world darkened, the result at the top, the scoreboard under it (`hud.rs`),
    // the button to play again at the bottom.
    let again = commands
        .spawn((
            Button,
            Node {
                width: px(260),
                height: px(46),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BorderColor::all(TRIM),
            BackgroundColor(BUTTON_GOLD),
            Tint(BUTTON_GOLD),
            Act::Queue,
            children![(text("PLAY AGAIN", 16.0, TEXT), outline())],
        ))
        .id();
    let result = commands
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(4),
            ..default()
        })
        .with_children(|r| {
            r.spawn((
                text("", 64.0, GOLD),
                TextShadow { offset: Vec2::splat(3.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
                Words::Result,
            ));
            r.spawn((
                Node { width: px(380), height: px(2), border_radius: BorderRadius::all(px(1)), ..default() },
                BackgroundColor(GOLD),
                Ribbon,
            ));
            r.spawn((text("", 18.0, TEXT), outline(), Words::Score));
            r.spawn((text("", 14.0, DIM), outline(), Words::Mine));
        })
        .id();
    let bottom = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: percent(9),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .add_child(again)
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::top(percent(5)),
                display: Display::None,
                ..default()
            },
            crate::hud::down(Color::srgba(0.0, 0.0, 0.0, 0.80), Color::srgba(0.0, 0.0, 0.0, 0.62)),
            // Under the scoreboard, over the rest of the HUD.
            GlobalZIndex(-5),
            Over,
        ))
        .add_children(&[result, bottom]);
}

/// The card while there's no match to show; the result once one is over.
fn show(
    session: Res<Session>,
    mut since: ResMut<Since>,
    mut card: Query<&mut Node, (With<Card>, Without<Over>)>,
    mut over: Query<&mut Node, (With<Over>, Without<Card>)>,
) {
    if since.phase != session.phase {
        since.phase = session.phase;
        since.at = crate::net::now_s();
    }
    let card_up = session.watch.is_none()
        && matches!(
            session.phase,
            Phase::Connecting
                | Phase::SigningIn
                | Phase::Lobby
                | Phase::Queued
                | Phase::Reconnecting
                | Phase::Failed
        );
    let over_up = session.phase == Phase::Over && session.watch.is_none();
    let flex = |up| if up { Display::Flex } else { Display::None };
    for mut n in &mut card {
        if n.display != flex(card_up) {
            n.display = flex(card_up);
        }
    }
    for mut n in &mut over {
        if n.display != flex(over_up) {
            n.display = flex(over_up);
        }
    }
}

fn words(
    session: Res<Session>,
    since: Res<Since>,
    mut q: Query<(&Words, &mut Text, &mut TextColor)>,
    mut ribbon: Query<&mut BackgroundColor, With<Ribbon>>,
) {
    let now = crate::net::now_s();
    let waited = (now - since.at).max(0.0) as u32;
    let won = match (session.winner, session.client.as_deref()) {
        (Some(w), _) if w > 1 => None,
        (Some(w), Some(c)) => Some(w == c.team),
        _ => None,
    };
    let result_colour = match won {
        Some(true) => GOLD,
        Some(false) => LOSS,
        None => DIM,
    };
    for mut bg in &mut ribbon {
        bg.set_if_neq(BackgroundColor(result_colour));
    }
    for (w, mut t, mut colour) in &mut q {
        let (want, tint): (String, Option<Color>) = match (*w, session.phase) {
            (Words::Title, Phase::Connecting) => ("CONNECTING".into(), None),
            (Words::Title, Phase::SigningIn) => ("SIGN IN".into(), None),
            (Words::Title, Phase::Lobby) => ("READY TO PLAY".into(), None),
            (Words::Title, Phase::Queued) => ("FINDING A MATCH".into(), None),
            (Words::Title, Phase::Reconnecting) => ("RECONNECTING".into(), None),
            (Words::Title, Phase::Failed) => ("DISCONNECTED".into(), Some(LOSS)),
            (Words::Title, _) => (String::new(), None),
            (Words::Big, Phase::Queued) => (format!("{}:{:02}", waited / 60, waited % 60), None),
            (Words::Big, _) => (String::new(), None),
            (Words::Line, Phase::Connecting) => ("Reaching the game server...".into(), None),
            (Words::Line, Phase::SigningIn) => ("Sign the message in your wallet to play.".into(), None),
            (Words::Line, Phase::Lobby) => ("Five against five on one lane, random champions.".into(), None),
            (Words::Line, Phase::Queued) => {
                let others = session.waiting.saturating_sub(1);
                let who = match others {
                    0 => "Just you so far".to_string(),
                    1 => "You and one other".to_string(),
                    n => format!("You and {n} others"),
                };
                let fill = (session.fill_at - now).max(0.0).ceil() as u32;
                let bots = if session.fill_at <= 0.0 {
                    String::new()
                } else if fill > 0 {
                    format!(".\nBots take the empty seats in {fill} s.")
                } else {
                    ".\nBots are taking the empty seats...".to_string()
                };
                (format!("{who}{bots}"), None)
            }
            (Words::Line, Phase::Reconnecting) => ("Taking your seat back...".into(), None),
            (Words::Line, Phase::Failed) => {
                (session.error.clone().unwrap_or_else(|| "The link to the server is gone.".into()), None)
            }
            (Words::Line, _) => (String::new(), None),
            (Words::Button, Phase::Lobby) => ("FIND A MATCH".into(), None),
            (Words::Button, Phase::Queued) => ("LEAVE THE QUEUE".into(), None),
            (Words::Button, Phase::Failed) => ("RELOAD".into(), None),
            (Words::Button, _) => (String::new(), None),
            (Words::Result, Phase::Over) => (
                match won {
                    Some(true) => "VICTORY".into(),
                    Some(false) => "DEFEAT".into(),
                    None if session.winner.is_some_and(|w| w > 1) => "NO CONTEST".into(),
                    None => "THE MATCH IS OVER".into(),
                },
                Some(result_colour),
            ),
            (Words::Score, Phase::Over) => {
                let kills = |team: u8| -> u32 {
                    (0..SEATS).filter(|&s| team_of_seat(s) == team).map(|s| u32::from(session.kd[s].0)).sum()
                };
                let clock = session.client.as_deref().map_or(String::new(), |c| mmss(c.world.tick));
                match session.winner {
                    Some(w) if w > 1 => ("The match was stopped.".into(), None),
                    _ => (format!("Dawn  {}  -  {}  Dusk      {clock}", kills(0), kills(1)), None),
                }
            }
            (Words::Mine, Phase::Over) => match session.client.as_deref() {
                Some(c) => {
                    let ch = &c.pred.champs[c.seat];
                    let (k, d) = session.kd[c.seat];
                    (
                        format!("{}   level {}   {k} / {d}   {} cs", ch.def().name, ch.level, ch.last_hits),
                        None,
                    )
                }
                None => (String::new(), None),
            },
            (Words::Result | Words::Score | Words::Mine, _) => (String::new(), None),
        };
        if t.as_str() != want {
            **t = want;
        }
        if let Some(c) = tint {
            colour.set_if_neq(TextColor(c));
        } else if *w == Words::Title {
            colour.set_if_neq(TextColor(GOLD));
        }
    }
}

/// The sweep, back and forth across the card while it waits on something.
fn sweep(session: Res<Session>, mut q: Query<(&mut Node, &mut Visibility), With<Sweep>>) {
    let waiting =
        matches!(session.phase, Phase::Connecting | Phase::SigningIn | Phase::Queued | Phase::Reconnecting);
    for (mut node, mut vis) in &mut q {
        vis.set_if_neq(if waiting { Visibility::Inherited } else { Visibility::Hidden });
        if !waiting {
            continue;
        }
        // Across in 1.4 s and back, eased; moved in steps of a percent.
        let t = (crate::net::now_s() / 1.4) % 2.0;
        let x = if t < 1.0 { t } else { 2.0 - t };
        let eased = x * x * (3.0 - 2.0 * x);
        let want = percent((eased * 70.0).round() as f32);
        if node.left != want {
            node.left = want;
        }
    }
}

/// The card's button: in the queue from the lobby, out of it while waiting, the page again once
/// the link is gone; hidden otherwise.
fn card_button(
    session: Res<Session>,
    mut q: Query<(&mut Act, &mut Node, &mut Tint, &mut BackgroundColor), With<CardButton>>,
) {
    let (act, base) = match session.phase {
        Phase::Lobby => (Some(Act::Queue), BUTTON_GOLD),
        Phase::Queued => (Some(Act::Leave), BUTTON),
        Phase::Failed => (Some(Act::Reload), BUTTON_GOLD),
        _ => (None, BUTTON),
    };
    for (mut a, mut node, mut tint, mut bg) in &mut q {
        let want = if act.is_some() { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
        if let Some(act) = act {
            if *a != act {
                *a = act;
            }
            if tint.0 != base {
                tint.0 = base;
                bg.0 = base;
            }
        }
    }
}

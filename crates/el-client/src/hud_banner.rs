//! The announcer's banner (`docs/FX.md`, "The voices"): the line said, shown as it's said, high in
//! the middle, as Riot pairs its kill callouts with a banner. Its colour is whose news it is (the
//! viewer's team's for good news, the other's for bad), and a kill names the champions in it. A
//! line without its recording is still shown: the banner is the announcer when there's no sound.

use bevy::prelude::*;
use el_client_core::fx::voice::Said;

use crate::dev_hooks::DevStatus;
use crate::hud::{DIM, TEXT, outline, text};
use crate::session::{Phase, Session};
use crate::view::TEAM;

/// How long a line is shown (s), if no other comes sooner; it fades over the last part.
const SHOWN: f64 = 2.8;
const FADE: f64 = 0.5;

/// The line being said, and when it was.
#[derive(Resource, Default)]
pub struct Banner {
    pub said: Option<Said>,
    pub at: f64,
    /// Lines shown, all told.
    pub shown: u32,
}

impl Banner {
    pub fn show(&mut self, said: Said, now: f64) {
        self.said = Some(said);
        self.at = now;
        self.shown += 1;
    }
}

#[derive(Component)]
struct Root;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Part {
    Line,
    Names,
}

pub struct BannerPlugin;

impl Plugin for BannerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Banner>()
            .add_systems(PostStartup, setup)
            .add_systems(Update, show.after(crate::fx::FxSet));
    }
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: percent(16),
            width: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(2),
            display: Display::None,
            ..default()
        },
        Root,
        children![
            (
                text("", 30.0, TEXT),
                TextLayout::justify(Justify::Center),
                TextShadow { offset: Vec2::splat(2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
                Part::Line
            ),
            (text("", 15.0, DIM), outline(), TextLayout::justify(Justify::Center), Part::Names),
        ],
    ));
}

/// The banner's words, shown or not, and their colour and fade.
fn show(
    session: Res<Session>,
    banner: Res<Banner>,
    mut root: Query<&mut Node, With<Root>>,
    mut parts: Query<(&Part, &mut Text, &mut TextColor)>,
    mut status: ResMut<DevStatus>,
) {
    let now = crate::net::now_s();
    let age = now - banner.at;
    let on_map =
        matches!(session.phase, Phase::Playing | Phase::Over | Phase::Watching | Phase::Reconnecting);
    let said = banner.said.filter(|_| on_map && age < SHOWN);
    status.set("banner", said.map_or("", |s| s.says()));
    status.set("banner_shown", banner.shown);
    for mut node in &mut root {
        let want = if said.is_some() { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    let Some(s) = said else { return };
    // The viewer's team: a match watched is seen from Dawn's side.
    let ours = usize::from(session.client.as_deref().map_or(0, |c| c.team).min(1));
    let colour = match s.good {
        Some(true) => TEAM[ours],
        Some(false) => TEAM[1 - ours],
        None => TEXT,
    };
    let alpha = (((SHOWN - age) / FADE).clamp(0.0, 1.0)) as f32;
    let champ = |seat: Option<u8>| seat.map(|k| champion_name(&session, usize::from(k)));
    let names = match (champ(s.killer), champ(s.victim)) {
        (Some(k), Some(v)) => format!("{k}  \u{203a}  {v}"),
        (None, Some(v)) => v.to_string(),
        (Some(k), None) => k.to_string(),
        (None, None) => String::new(),
    };
    let line = s.says().trim_end_matches(['.', '!']).to_uppercase();
    for (part, mut t, mut c) in &mut parts {
        let (want, tint) = match part {
            Part::Line => (line.as_str(), colour),
            Part::Names => (names.as_str(), DIM),
        };
        if **t != *want {
            **t = want.to_string();
        }
        let tint = tint.with_alpha(tint.alpha() * alpha);
        if c.0 != tint {
            c.0 = tint;
        }
    }
}

/// The name of the champion in `seat` (the banner names champions, as Riot's portraits do).
fn champion_name(session: &Session, seat: usize) -> &'static str {
    let champs = match (&session.watch, &session.client) {
        (Some(w), _) => &w.referee.m.champs,
        (None, Some(c)) => &c.world.champs,
        (None, None) => return "",
    };
    champs.get(seat).map_or("", |c| c.def().name)
}

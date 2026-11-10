//! The main menu (`docs/DESIGN.md`, "The menu"): Gates' front end (`AnthonE/Gates`,
//! `render/menu.rs`, MIT, MoreRight DAO) in Eloram's colours, on the browser and the desktop alike,
//! drawn in the game's own UI. A wordmark top left, the identity at the top right, a nav column
//! that never moves, a panel and a pane over the Causeway drifting under a scrim; PLAY (normal or
//! ranked), PRACTICE, WATCH, FRIENDS, NEWS, STORE, PROFILE, OPTIONS (the Esc menu's pages) and,
//! on the desktop, QUIT. The words and the decisions are `el_client_core::front`'s, tested there.
//!
//! It's a screen, not a phase: the session is `Lobby` under it (welcomed, not queued), as before,
//! and goes down as a match (or a watch) starts. A launch with a deep link (`?queue=1`,
//! `?practice=1`, `?watch=`, `?replay=`, `?fxstage=`, `?bench=1`, `?mode=echo`; the desktop's
//! `--queue`, `--practice`, `--watch`) skips it. While it's up the world isn't drawn: one window
//! camera at a time (the menu's own 2D one), as Gates draws it.

pub mod ui;

use bevy::prelude::*;
use el_client_core::front::hub::{Section, Status};
use el_client_core::front::manifest::Manifest;
use el_client_core::front::play::{PlayInputs, play_view};
use el_client_core::front::records::WatchRow;
use el_client_core::front::who::{Source, Who};
use el_client_core::front::{Nav, PlayMode};
use el_client_core::lobby::ToServer;
use el_sim::content::champions::CHAMPIONS;

use self::ui::*;
use crate::camera::MainCamera;
use crate::dev_hooks::DevStatus;
use crate::hud::{DIM, GOLD, TEXT};
use crate::input::Controls;
use crate::net::LaunchConfigRes;
use crate::platform::{Opened, Req, Slot, now_s};
use crate::session::{Phase, Session};

/// The manifest's address when the launcher doesn't name one.
const MANIFEST_URL: &str = "https://elopros.com/api/launcher/manifest/eloram";
/// How often WATCH asks the server what's on.
const ROWS_EVERY: f64 = 10.0;

/// The menu's state.
#[derive(Resource)]
pub struct Front {
    /// Up: drawn, the world under it not.
    pub up: bool,
    pub nav: Nav,
    pub mode: PlayMode,
    /// The last line a click left (an open, a refusal), shown in the footer.
    pub status: String,
    pub who: Who,
    /// The server holds practice and ranked matches (it says so when it welcomes us).
    pub practice_here: bool,
    pub ranked_here: bool,
    /// The ranked queue's state, as the server relays the store's.
    pub ranked_state: Option<String>,
    manifest: Option<Result<Manifest, String>>,
    manifest_req: Option<Slot>,
    pub rows: Vec<WatchRow>,
    rows_req: Option<Slot>,
    rows_at: f64,
    rows_error: Option<String>,
    /// What the screen was last built from: rebuilt only when this changes.
    drawn: String,
    queued_at: Option<f64>,
    /// `?watch=live`: the newest live match, once the records are in; the desktop's
    /// `--watch <id>`: that record, once the server's found.
    watch_newest: bool,
    watch_id: Option<String>,
}

impl Front {
    fn new(up: bool) -> Self {
        Front {
            up,
            nav: Nav::Play,
            mode: PlayMode::Normal,
            status: String::new(),
            who: Who::default(),
            practice_here: false,
            ranked_here: false,
            ranked_state: None,
            manifest: None,
            manifest_req: None,
            rows: Vec::new(),
            rows_req: None,
            rows_at: f64::NEG_INFINITY,
            rows_error: None,
            drawn: String::new(),
            queued_at: None,
            watch_newest: false,
            watch_id: None,
        }
    }

    /// Back to the menu: what the last world left goes (`Session::begin_world`), the phase is the
    /// lobby's again.
    pub fn back(&mut self, session: &mut Session) {
        session.begin_world();
        session.client = None;
        session.watch = None;
        if matches!(session.phase, Phase::Over | Phase::Watching | Phase::Playing | Phase::Selecting) {
            session.phase = if session.session.is_some() { Phase::Lobby } else { Phase::Connecting };
        }
        self.up = true;
        self.nav = Nav::Play;
        // Watching dialled nothing: the menu's PLAY wants the server.
        if session.session.is_none() {
            crate::net::DIAL_NOW.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// What a click asks for.
#[derive(Component, Clone, Debug)]
enum Pick {
    Nav(Nav),
    Mode(PlayMode),
    Go,
    Practice(u8),
    Watch(usize),
    Refresh,
    Open(Section),
    /// The player's profile on Elo Pros (their face, hours, rating and friends).
    Profile,
    /// Into the party this leader asked us into; not; out of ours.
    PartyJoin(u32),
    PartyDecline(u32),
    PartyLeave,
}

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuCamera;

#[derive(Component)]
struct NavLabel(Nav);

pub struct FrontPlugin;

impl Plugin for FrontPlugin {
    fn build(&self, app: &mut App) {
        ui::build(app);
        let cfg = &app.world().resource::<LaunchConfigRes>().0;
        let mut front = Front::new(cfg.menu());
        front.watch_newest = cfg.watch_newest || cfg.record_id.is_some();
        front.watch_id = cfg.record_id.clone();
        app.insert_resource(front).add_systems(Startup, load_backdrop).add_systems(
            Update,
            (
                steer,
                refusals,
                fetches,
                newest,
                click,
                keys,
                rebuild,
                ui::hover,
                nav_hover,
                drift,
                cameras,
                top_button,
                publish,
            )
                .chain()
                .after(crate::net::NetSet::Receive)
                .before(crate::net::NetSet::Send),
        );
    }
}

#[derive(Resource)]
struct Backdrop(Handle<Image>);

fn load_backdrop(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(Backdrop(server.load(ui::BACKDROP_PATH)));
}

/// `?watch=live`: the newest live match, once `records.json` names one (asked again every ten
/// seconds until it does).
fn newest(mut front: ResMut<Front>, mut session: ResMut<Session>, map: Res<crate::net::MapRes>) {
    if !front.watch_newest {
        return;
    }
    let id = front.watch_id.clone();
    let Some(row) = front.rows.iter().find(|r| id.as_ref().map_or(r.live, |i| &r.id == i)).cloned() else {
        return;
    };
    let Some(url) = crate::config::server_url(&format!("records/{}", row.id)) else { return };
    front.watch_newest = false;
    crate::watch::start_url(&mut session, map.0, url, true);
}

/// The Esc menu's button (top left) steps aside while the main menu is up: its OPTIONS is there.
/// Only as the menu goes up or down: the fx stage hides the button too (`stage.rs`).
fn top_button(
    front: Res<Front>,
    mut was: Local<Option<bool>>,
    mut q: Query<&mut Visibility, With<crate::menu::TopButton>>,
) {
    if *was == Some(front.up) {
        return;
    }
    if was.is_none() && !front.up {
        *was = Some(false);
        return;
    }
    *was = Some(front.up);
    let want = if front.up { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut q {
        if *v != want {
            *v = want;
        }
    }
}

/// Down as a match or a watch starts; the queue's clock kept.
fn steer(mut front: ResMut<Front>, session: Res<Session>) {
    if front.up
        && matches!(
            session.phase,
            Phase::Selecting | Phase::Playing | Phase::Reconnecting | Phase::Watching | Phase::Stage
        )
    {
        front.up = false;
    }
    match (session.phase, front.queued_at) {
        (Phase::Queued, None) => front.queued_at = Some(now_s()),
        (Phase::Queued, Some(_)) => {}
        (_, Some(_)) => front.queued_at = None,
        _ => {}
    }
    let signed = session.signed_in;
    if !signed && front.who.id.is_some() && session.session.is_some() {
        front.who = Who { streamer: front.who.streamer, ..Who::default() };
    }
    if signed && front.who.source == Source::Guest {
        front.who.source = if crate::platform::DESKTOP { Source::Launcher } else { Source::Browser };
    }
    // Who the server knows us as, and what it holds (`ToClient::About`).
    if session.player.is_some() && front.who.id != session.player {
        front.who.id = session.player.clone();
        front.who.name = None;
    }
    if front.practice_here != session.practice_here {
        front.practice_here = session.practice_here;
    }
    if front.ranked_here != session.ranked_here {
        front.ranked_here = session.ranked_here;
    }
    if front.ranked_state != session.ranked_line {
        front.ranked_state = session.ranked_line.clone();
    }
}

/// What the server turned down, on the status line (and the menu back up to show it).
fn refusals(mut front: ResMut<Front>, mut session: ResMut<Session>) {
    if std::mem::take(&mut session.left) {
        front.up = true;
        front.nav = Nav::Play;
    }
    if let Some(why) = session.refused.take() {
        front.status = why;
        if session.phase == Phase::Lobby {
            front.up = true;
        }
    }
}

/// The manifest (once), and the records while WATCH is open (or `?watch=live` looks).
fn fetches(mut front: ResMut<Front>) {
    if !front.up && !front.watch_newest {
        return;
    }
    if front.manifest.is_none() && front.manifest_req.is_none() {
        front.manifest_req = Some(crate::platform::fetch(Req::get(MANIFEST_URL).cap(64 * 1024)));
    }
    if let Some(a) = front.manifest_req.as_ref().and_then(Slot::take) {
        front.manifest_req = None;
        front.manifest = Some(a.and_then(|b| Manifest::parse(&b)));
    }
    let now = now_s();
    let looking = (front.up && front.nav == Nav::Watch) || front.watch_newest;
    if looking && front.rows_req.is_none() && now - front.rows_at > ROWS_EVERY {
        front.rows_at = now;
        match crate::config::server_url("records.json") {
            Some(url) => front.rows_req = Some(crate::platform::fetch(Req::get(url).cap(1 << 20))),
            None => front.rows_error = Some("The game server isn't found yet.".into()),
        }
    }
    if let Some(a) = front.rows_req.as_ref().and_then(Slot::take) {
        front.rows_req = None;
        match a {
            Ok(b) => {
                front.rows = el_client_core::front::records::rows(&b);
                front.rows_error = None;
            }
            Err(e) => front.rows_error = Some(e),
        }
    }
}

fn hub_status(front: &Front, s: Section) -> Status {
    // The browser opens a tab itself: it always has a way to open a page.
    let can_open = !crate::platform::DESKTOP || crate::launcher_running();
    Status::of(can_open, front.manifest.as_ref(), s)
}

fn play_inputs(front: &Front, session: &Session) -> PlayInputs {
    PlayInputs {
        linked: session.session.is_some(),
        connecting: matches!(session.phase, Phase::Connecting | Phase::SigningIn),
        failed: session.phase == Phase::Failed,
        wallet: front.who.wallet() || (session.signed_in && front.who.id.is_none()),
        party_member: session.party.on() && Some(session.party.leader) != session.session,
        ranked_here: front.ranked_here,
        queued: (session.phase == Phase::Queued).then_some(front.mode),
        mode: front.mode,
    }
}

#[allow(clippy::too_many_arguments)]
fn act(
    pick: Pick,
    map: &'static el_sim::map::Map,
    front: &mut Front,
    session: &mut Session,
    controls: &mut Controls,
    menu: &mut crate::menu::Menu,
    exit: &mut MessageWriter<AppExit>,
) {
    match pick {
        Pick::Nav(Nav::Options) => menu.show(true),
        Pick::Nav(Nav::Quit) => {
            exit.write(AppExit::Success);
        }
        Pick::Nav(n) => {
            front.nav = n;
            if n == Nav::Watch {
                front.rows_at = f64::NEG_INFINITY;
            }
        }
        Pick::Mode(m) => {
            if session.phase != Phase::Queued {
                front.mode = m;
            }
        }
        Pick::Go => {
            let view = play_view(play_inputs(front, session));
            if !view.ready {
                front.status = view.line;
                return;
            }
            match (session.phase, view.label) {
                (Phase::Queued, _) => controls.lobby.push(ToServer::Leave),
                (Phase::Failed, _) => crate::platform::reload(),
                (_, "SIGN IN") => crate::net::ask_sign_in(),
                _ => controls.lobby.push(match front.mode {
                    PlayMode::Normal => ToServer::Queue,
                    PlayMode::Ranked => ToServer::QueueRanked,
                }),
            }
        }
        Pick::Practice(champ) => {
            if session.session.is_some() {
                controls.lobby.push(ToServer::Practice { champ });
            }
        }
        Pick::Watch(i) => {
            let Some(row) = front.rows.get(i).cloned() else { return };
            let Some(url) = crate::config::server_url(&format!("records/{}", row.id)) else { return };
            crate::watch::start_url(session, map, url, row.live);
            front.up = false;
        }
        Pick::Refresh => front.rows_at = f64::NEG_INFINITY,
        Pick::PartyJoin(leader) => controls.lobby.push(ToServer::PartyAccept { leader }),
        Pick::PartyDecline(leader) => controls.lobby.push(ToServer::PartyDecline { leader }),
        Pick::PartyLeave => controls.lobby.push(ToServer::PartyLeave),
        Pick::Profile => {
            let Some(id) = front.who.id.as_deref() else {
                front.status = "Sign in to see your Elo Pros profile.".into();
                return;
            };
            let url = el_client_core::front::who::profile_url(id);
            front.status = match crate::platform::open_url(&url) {
                Opened::Opened => "Opened your Elo Pros profile in your browser.".into(),
                Opened::Refused => "The browser didn't open it (a pop-up blocked?).".into(),
                Opened::NoLauncher => {
                    "The Elo launcher isn't running: start Eloram from it to open this.".into()
                }
            };
        }
        Pick::Open(s) => {
            let Status::Ready(url) = hub_status(front, s) else {
                front.status = hub_status(front, s).line();
                return;
            };
            let got = crate::platform::open_url(&url);
            front.status =
                el_client_core::front::hub::opened_line(s, got == Opened::Opened, got == Opened::Refused);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn click(
    q: Query<(&Interaction, &Pick), Changed<Interaction>>,
    map: Res<crate::net::MapRes>,
    mut front: ResMut<Front>,
    mut session: ResMut<Session>,
    mut controls: ResMut<Controls>,
    mut menu: ResMut<crate::menu::Menu>,
    mut exit: MessageWriter<AppExit>,
) {
    if !front.up || menu.open {
        return;
    }
    let picks: Vec<Pick> =
        q.iter().filter(|(i, _)| **i == Interaction::Pressed).map(|(_, p)| p.clone()).collect();
    for p in picks {
        act(p, map.0, &mut front, &mut session, &mut controls, &mut menu, &mut exit);
    }
}

/// Esc: back to PLAY from another page (on PLAY it does nothing: QUIT is the way out); Enter plays.
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<crate::net::MapRes>,
    mut front: ResMut<Front>,
    mut session: ResMut<Session>,
    mut controls: ResMut<Controls>,
    mut menu: ResMut<crate::menu::Menu>,
    mut exit: MessageWriter<AppExit>,
) {
    if !front.up || menu.open {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) && front.nav != Nav::Play {
        front.nav = Nav::Play;
    }
    if (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter))
        && front.nav == Nav::Play
    {
        act(Pick::Go, map.0, &mut front, &mut session, &mut controls, &mut menu, &mut exit);
    }
}

/// What the screen shows, as one string: rebuilt when it changes (a click, the queue's second,
/// the rows coming in), never each frame.
fn key(front: &Front, session: &Session) -> String {
    let queued = front.queued_at.map_or(0, |t| (now_s() - t) as u32);
    format!(
        "{:?}|{:?}|{:?}|{}|{}|{}|{}|{:?}|{}|{:?}|{:?}|{}|{}|{}|{}",
        front.nav,
        front.mode,
        session.phase,
        session.waiting,
        queued,
        front.status,
        front.rows.len(),
        front.rows_error,
        front.who.shown_name(),
        front.manifest.as_ref().map(|m| m.is_ok()),
        front.ranked_state,
        front.practice_here,
        front.ranked_here,
        session.signed_in,
        session.error.as_deref().unwrap_or(""),
    ) + &format!("|{:?}", session.party)
}

#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    mut front: ResMut<Front>,
    session: Res<Session>,
    roots: Query<Entity, With<MenuRoot>>,
    backdrop: Option<Res<Backdrop>>,
) {
    if !front.up {
        if !roots.is_empty() {
            for r in &roots {
                commands.entity(r).despawn();
            }
            front.drawn.clear();
        }
        return;
    }
    let k = key(&front, &session);
    if k == front.drawn && !roots.is_empty() {
        return;
    }
    front.drawn = k;
    for r in &roots {
        commands.entity(r).despawn();
    }
    let root = commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(INK),
            GlobalZIndex(30),
        ))
        .id();
    if let Some(b) = backdrop {
        let d = commands.spawn(ui::backdrop(b.0.clone())).id();
        commands.entity(root).add_child(d);
    }
    let s = commands.spawn(ui::scrim()).id();
    commands.entity(root).add_child(s);
    let header = header(&mut commands, &front);
    let body = body(&mut commands, &front, &session);
    let footer = footer(&mut commands, &front);
    commands.entity(root).add_children(&[header, body, footer]);
}

fn header(c: &mut Commands, front: &Front) -> Entity {
    let who = &front.who;
    let initial = who.shown_name().chars().next().unwrap_or('?').to_ascii_uppercase().to_string();
    c.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            padding: UiRect::new(px(GUT), px(GUT), px(35), px(25)),
            ..default()
        },
        children![
            ui::wordmark("FIVE AGAINST FIVE ON ONE LANE"),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(14),
                    ..default()
                },
                children![
                    (
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::End,
                            row_gap: px(3),
                            ..default()
                        },
                        children![strong(who.shown_name(), 20.0, TITLE), label(who.line(), 15.0, DIM)],
                    ),
                    (
                        Node {
                            width: px(50),
                            height: px(50),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1)),
                            ..default()
                        },
                        BackgroundColor(ROW_HOVER),
                        BorderColor::all(RULE),
                        children![strong(initial, 22.0, GOLD)],
                    ),
                ],
            ),
        ],
    ))
    .id()
}

fn body(c: &mut Commands, front: &Front, session: &Session) -> Entity {
    let nav = c
        .spawn(Node {
            width: px(NAV_W),
            flex_direction: FlexDirection::Column,
            padding: UiRect::top(px(8)),
            flex_shrink: 0.0,
            ..default()
        })
        .id();
    for n in Nav::all(crate::platform::DESKTOP) {
        if n.gap_before() {
            let g = c.spawn(Node { height: px(22), ..default() }).id();
            c.entity(nav).add_child(g);
        }
        let item = c.spawn((ui::nav_item(n.label(), front.nav == n), Pick::Nav(n), NavLabel(n))).id();
        c.entity(nav).add_child(item);
    }
    let row = c
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            flex_grow: 1.0,
            min_height: px(0),
            padding: UiRect::horizontal(px(GUT)),
            column_gap: px(0),
            ..default()
        })
        .id();
    c.entity(row).add_child(nav);
    let pages = match front.nav {
        Nav::Play => play_page(c, front, session),
        Nav::Practice => practice_page(c, front, session),
        Nav::Watch => watch_page(c, front),
        Nav::News => hub_page(c, front, Section::News),
        Nav::Store => hub_page(c, front, Section::Store),
        Nav::Friends | Nav::Profile => soon_page(c, front),
        Nav::Options | Nav::Quit => play_page(c, front, session),
    };
    c.entity(row).add_children(&pages);
    row
}

fn title_and_blurb(c: &mut Commands, panel: Entity, title: &str, blurb: &str) {
    let t = c.spawn(strong(title.to_string(), 32.0, TITLE)).id();
    let b = c.spawn(label(blurb.to_string(), 17.0, DIM)).id();
    c.entity(panel).add_children(&[t, b]);
}

fn play_page(c: &mut Commands, front: &Front, session: &Session) -> Vec<Entity> {
    let panel = c.spawn(ui::panel(PANEL_W)).id();
    title_and_blurb(c, panel, "PLAY", "Random champions, one lane, a match that ends.");
    let rule = c.spawn(ui::group_rule("MODE")).id();
    c.entity(panel).add_child(rule);
    for m in [PlayMode::Normal, PlayMode::Ranked] {
        let on = front.mode == m;
        let b = c
            .spawn((
                Button,
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(12)),
                    row_gap: px(4),
                    border: UiRect::all(px(1)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(if on { PICKED } else { Color::NONE }),
                BorderColor::all(if on { crate::hud::TRIM } else { RULE }),
                Hover { idle: if on { PICKED } else { Color::NONE }, over: ROW_HOVER },
                Pick::Mode(m),
                children![strong(m.label(), 20.0, if on { GOLD } else { TEXT }), label(m.blurb(), 15.0, DIM)],
            ))
            .id();
        c.entity(panel).add_child(b);
    }
    party_card(c, panel, session);
    let gap = c.spawn(ui::spacer()).id();
    let view = play_view(play_inputs(front, session));
    let go = c.spawn((ui::go(view.label, &view.line, view.ready), Pick::Go)).id();
    c.entity(panel).add_children(&[gap, go]);
    if session.phase == Phase::Failed
        && let Some(e) = &session.error
    {
        let l = c.spawn(label(e.clone(), 15.0, BAD)).id();
        c.entity(panel).add_child(l);
    }
    // The stage: what the queue is doing, or what the mode is.
    let stage = c
        .spawn((Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::End,
            padding: UiRect::new(px(60), px(0), px(0), px(10)),
            row_gap: px(8),
            ..default()
        },))
        .id();
    let (kicker, head, line) = match session.phase {
        Phase::Queued => {
            let waited = front.queued_at.map_or(0.0, |t| now_s() - t) as u32;
            let others = session.waiting.saturating_sub(1);
            let line = match (front.mode, &front.ranked_state) {
                (PlayMode::Ranked, Some(s)) => format!("The Elo Pros queue: {s}"),
                (PlayMode::Ranked, None) => "Waiting for ten wallets.".to_string(),
                (PlayMode::Normal, _) => {
                    let fill = (session.fill_at - now_s()).max(0.0) as u32;
                    format!("{others} others waiting. Bots take the empty seats in {fill} s.")
                }
            };
            (front.mode.label(), format!("FINDING A MATCH  {}:{:02}", waited / 60, waited % 60), line)
        }
        _ => (front.mode.label(), "THE CAUSEWAY".to_string(), front.mode.blurb().to_string()),
    };
    let k = c
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(10),
                ..default()
            },
            children![
                (Node { width: px(32), height: px(4), ..default() }, BackgroundColor(GOLD)),
                strong(kicker, 15.0, TITLE)
            ],
        ))
        .id();
    let h = c.spawn((strong(head, 44.0, TITLE), TEXT_SHADOW)).id();
    let l = c.spawn((label(line, 18.0, TEXT), TEXT_SHADOW)).id();
    c.entity(stage).add_children(&[k, h, l]);
    vec![panel, stage]
}

/// The party on PLAY: who's in it, the asks into others', and the way out.
fn party_card(c: &mut Commands, panel: Entity, session: &Session) {
    let p = &session.party;
    if !p.on() && p.invites_in.is_empty() {
        return;
    }
    let rule = c.spawn(ui::group_rule("PARTY")).id();
    c.entity(panel).add_child(rule);
    if p.on() {
        for m in &p.members {
            let lead = if m.session == p.leader { "  LEADER" } else { "" };
            let me = if Some(m.session) == session.session { "  (you)" } else { "" };
            let l = c.spawn(label(format!("{}{lead}{me}", m.name), 16.0, TEXT)).id();
            c.entity(panel).add_child(l);
        }
        let line = if Some(p.leader) == session.session {
            "You lead: PLAY queues the party, on one side."
        } else {
            "The leader queues for the party."
        };
        let l = c.spawn(label(line, 14.0, DIM)).id();
        let out = c.spawn((ui::button("LEAVE PARTY", 0.0, true), Pick::PartyLeave)).id();
        c.entity(panel).add_children(&[l, out]);
    }
    for ask in &p.invites_in {
        let row = c
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(8),
                ..default()
            })
            .id();
        let l = c
            .spawn((
                label(format!("{} asks you in", ask.name), 15.0, TEXT),
                Node { flex_grow: 1.0, ..default() },
            ))
            .id();
        let yes = c.spawn((ui::button("JOIN", 90.0, true), Pick::PartyJoin(ask.leader))).id();
        let no = c.spawn((ui::button("DECLINE", 110.0, true), Pick::PartyDecline(ask.leader))).id();
        c.entity(row).add_children(&[l, yes, no]);
        c.entity(panel).add_child(row);
    }
}

fn practice_page(c: &mut Commands, front: &Front, session: &Session) -> Vec<Entity> {
    let panel = c.spawn(ui::panel(PANEL_W)).id();
    title_and_blurb(
        c,
        panel,
        "PRACTICE",
        "Any champion, every rank, nothing on cooldown, at the dummies in the Causeway's middle. No minions, no one else.",
    );
    let line = if session.session.is_none() {
        "Reaching the game server..."
    } else if !front.practice_here {
        "This server holds no practice matches."
    } else {
        "Pick a champion to start."
    };
    let l = c.spawn(label(line, 15.0, if front.practice_here { DIM } else { BAD })).id();
    c.entity(panel).add_child(l);
    let pane = c.spawn(ui::pane(false)).id();
    let grid = c
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(10),
            row_gap: px(10),
            ..default()
        })
        .id();
    for (i, ch) in CHAMPIONS.iter().enumerate() {
        let b = c.spawn((ui::button(ch.name, 170.0, false), Pick::Practice(i as u8))).id();
        c.entity(grid).add_child(b);
    }
    c.entity(pane).add_child(grid);
    vec![panel, pane]
}

fn watch_page(c: &mut Commands, front: &Front) -> Vec<Entity> {
    let panel = c.spawn(ui::panel(PANEL_W)).id();
    title_and_blurb(
        c,
        panel,
        "WATCH",
        "Any match on this server: a live one a minute behind, or a finished one from its record, re-run to the server's own hashes.",
    );
    let r = c.spawn((ui::button("REFRESH", 0.0, false), Pick::Refresh)).id();
    c.entity(panel).add_child(r);
    let pane = c.spawn(ui::pane(false)).id();
    if let Some(e) = &front.rows_error {
        let l = c.spawn(label(format!("Couldn't ask the server: {e}"), 16.0, BAD)).id();
        c.entity(pane).add_child(l);
    } else if front.rows.is_empty() {
        let l = c.spawn(label("No matches yet.", 16.0, DIM)).id();
        c.entity(pane).add_child(l);
    }
    for (i, row) in front.rows.iter().take(14).enumerate() {
        let what = match (row.live, row.winner) {
            (true, _) => format!("LIVE  {} min", row.minutes),
            (false, Some(0)) => format!("DAWN WON  {} min", row.minutes),
            (false, Some(_)) => format!("DUSK WON  {} min", row.minutes),
            (false, None) => format!("{} min", row.minutes),
        };
        let picks: Vec<&str> =
            row.picks.iter().filter_map(|p| CHAMPIONS.get(usize::from(*p)).map(|c| c.name)).collect();
        let r = c
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: px(14),
                    padding: UiRect::axes(px(10), px(6)),
                    border: UiRect::bottom(px(1)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BorderColor::all(RULE),
            ))
            .id();
        let b = c
            .spawn((ui::button(if row.live { "WATCH" } else { "REPLAY" }, 110.0, row.live), Pick::Watch(i)))
            .id();
        let t = c
            .spawn((
                Node { flex_direction: FlexDirection::Column, row_gap: px(2), ..default() },
                children![
                    strong(format!("{what}{}", if row.ranked { "  RANKED" } else { "" }), 17.0, TITLE),
                    label(
                        if picks.is_empty() { format!("match {}", row.id) } else { picks.join(", ") },
                        14.0,
                        DIM
                    ),
                ],
            ))
            .id();
        c.entity(r).add_children(&[b, t]);
        c.entity(pane).add_child(r);
    }
    vec![panel, pane]
}

fn hub_page(c: &mut Commands, front: &Front, s: Section) -> Vec<Entity> {
    let panel = c.spawn(ui::panel(PANEL_W)).id();
    title_and_blurb(c, panel, s.label(), s.blurb());
    let rule = c.spawn(ui::group_rule("WHERE THIS LIVES")).id();
    let st = hub_status(front, s);
    let line = c.spawn(label(st.line(), 15.0, FAINT)).id();
    let gap = c.spawn(ui::spacer()).id();
    let why = c.spawn(label(s.why_elsewhere(), 14.0, FAINT)).id();
    let open = c
        .spawn((ui::button("OPEN IN YOUR BROWSER", 0.0, matches!(st, Status::Ready(_))), Pick::Open(s)))
        .id();
    c.entity(panel).add_children(&[rule, line, gap, why, open]);
    let pane = c.spawn(ui::pane(false)).id();
    let l = c
        .spawn(label(
            format!(
                "{} is part of Elo Pros, where Eloram is listed. It opens there rather than here.",
                s.label()
            ),
            17.0,
            DIM,
        ))
        .id();
    c.entity(pane).add_child(l);
    vec![panel, pane]
}

fn soon_page(c: &mut Commands, front: &Front) -> Vec<Entity> {
    let panel = c.spawn(ui::panel(PANEL_W)).id();
    let (t, b) = match front.nav {
        Nav::Friends => ("FRIENDS", "Your Elo Pros friends: who's online and in a match, invites and asks."),
        _ => ("PROFILE", "Your name and face on Elo Pros, your hours, your rating and the ladder."),
    };
    title_and_blurb(c, panel, t, b);
    let signed = front.who.id.is_some();
    let line = if signed {
        "They live on your Elo Pros profile, which opens in your browser."
    } else {
        "Sign in to see this: a guest has no profile."
    };
    let l = c.spawn(label(line, 15.0, if signed { FAINT } else { DIM })).id();
    let gap = c.spawn(ui::spacer()).id();
    let open = c.spawn((ui::button("OPEN YOUR PROFILE", 0.0, signed), Pick::Profile)).id();
    c.entity(panel).add_children(&[l, gap, open]);
    vec![panel]
}

fn footer(c: &mut Commands, front: &Front) -> Entity {
    c.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(22),
            align_items: AlignItems::Center,
            padding: UiRect::new(px(GUT), px(GUT), px(14), px(18)),
            ..default()
        },
        children![
            strong(format!("Eloram {}", crate::build_name()), 14.0, FAINT),
            strong("Elo Pros", 14.0, DIM),
            ui::spacer(),
            label(front.status.clone(), 14.0, DIM),
            strong(
                if front.nav == Nav::Play { "Enter plays  /  Esc menus" } else { "Esc goes back" },
                14.0,
                FAINT
            ),
        ],
    ))
    .id()
}

/// A nav entry under the pointer lifts its colour (the picked one stays brightest).
fn nav_hover(
    front: Res<Front>,
    q: Query<(&Interaction, &NavLabel, &Children), Changed<Interaction>>,
    mut texts: Query<&mut TextColor>,
) {
    for (i, n, kids) in &q {
        let over = matches!(i, Interaction::Hovered | Interaction::Pressed);
        if let Some(&t) = kids.get(1)
            && let Ok(mut col) = texts.get_mut(t)
        {
            col.0 = ui::nav_color(front.nav == n.0, over);
        }
    }
}

fn drift(time: Res<Time<Real>>, mut q: Query<&mut UiTransform, With<ui::Drifts>>) {
    let t = time.elapsed_secs();
    for mut x in &mut q {
        *x = ui::drift(t);
    }
}

/// One window camera at a time: the menu's own while it's up (the world isn't drawn under it),
/// the world's again once it's down.
fn cameras(
    mut commands: Commands,
    front: Res<Front>,
    mut main: Query<&mut Camera, (With<MainCamera>, Without<MenuCamera>)>,
    menu_cam: Query<Entity, With<MenuCamera>>,
) {
    let have = !menu_cam.is_empty();
    if front.up && !have {
        commands.spawn((
            MenuCamera,
            Camera2d,
            Camera { order: 20, clear_color: ClearColorConfig::Custom(INK), ..default() },
            Msaa::Off,
            IsDefaultUiCamera,
        ));
    } else if !front.up && have {
        for e in &menu_cam {
            commands.entity(e).despawn();
        }
    }
    for mut cam in &mut main {
        if cam.is_active == front.up {
            cam.is_active = !front.up;
        }
    }
}

fn publish(front: Res<Front>, session: Res<Session>, mut dev: ResMut<DevStatus>) {
    dev.set("front", front.up);
    dev.set("front_nav", front.nav.key());
    dev.set("front_mode", front.mode.key());
    dev.set("identity", front.who.id.clone().unwrap_or_default());
    dev.set("identity_name", front.who.shown_name());
    dev.set("practice_available", front.practice_here);
    dev.set("ranked_available", front.ranked_here);
    dev.set("ranked_state", front.ranked_state.clone().unwrap_or_default());
    dev.set("watch_rows", front.rows.len() as u32);
    dev.set("party_size", session.party.members.len() as u32);
    dev.set("party_leader", session.party.leader);
    dev.set("party_asks", session.party.invites_in.len() as u32);
    dev.set("watch_error", front.rows_error.clone().unwrap_or_default());
    dev.set("hub_news", hub_status(&front, Section::News).key());
    dev.set("hub_store", hub_status(&front, Section::Store).key());
    dev.set("front_status", front.status.clone());
    dev.set("play_label", play_view(play_inputs(&front, &session)).label);
}

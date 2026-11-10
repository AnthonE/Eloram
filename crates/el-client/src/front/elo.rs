//! FRIENDS and PROFILE: what Elo Pros keeps for the player, read into the menu
//! (`el_client_core::elopros` parses and builds the texts). Reads are plain GETs on
//! `https://elopros.com/api`; a change to the friends list is prepare → check the offered text →
//! sign it (the page's wallet, or the Elo launcher on the desktop) → act. Every state is said: the
//! page never shows a zero it didn't read.

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use el_client_core::elopros::friends::Op;
use el_client_core::elopros::profile::Read;
use el_client_core::elopros::{self, face, friends, profile};

use super::ui::{self, BAD, FAINT, PANEL_W, RULE, TITLE, label, strong};
use super::{Front, Nav, Pick, title_and_blurb};
use crate::hud::{DIM, GOLD, TEXT};
use crate::platform::{Req, Slot, now_s};

/// How often an open FRIENDS reads the list again (presence lags the servers by about this).
const FRIENDS_EVERY: f64 = 90.0;
/// How often an open PROFILE reads again.
const PROFILE_EVERY: f64 = 120.0;
/// A list row's most, drawn.
const ROWS: usize = 12;

/// A change on its way.
enum Stage {
    Prepare(Slot),
    Sign(Slot),
    Act(Slot),
}

struct Act {
    op: Op,
    other: String,
    stage: Stage,
    /// The text signed and its `at`, once checked.
    at: u64,
}

/// What the menu has of Elo Pros.
#[derive(Default)]
pub struct EloState {
    /// Whose (the player's id) the reads below are.
    for_id: Option<String>,
    friends: Read<friends::View>,
    friends_req: Option<Slot>,
    friends_at: f64,
    face: Read<face::Face>,
    face_req: Option<Slot>,
    pic_req: Option<Slot>,
    /// The player's picture, once read.
    pub pic: Option<Handle<Image>>,
    hours: Read<profile::Hours>,
    rating: Read<profile::Rating>,
    stats: Read<profile::Stats>,
    profile_reqs: Option<[Slot; 3]>,
    profile_at: f64,
    act: Option<Act>,
    /// What the last change came to.
    pub line: String,
    /// REMOVE pressed once on this friend: again removes.
    confirm: Option<String>,
}

/// What a click on FRIENDS asks for.
#[derive(Clone, Debug)]
pub enum FriendPick {
    Change(Op, String),
    /// REMOVE: twice.
    Remove(String),
    /// Into the party (the server finds them, if they're on it).
    Party(String),
    Refresh,
}

impl EloState {
    /// Everything that changes what the pages draw.
    pub fn key(&self) -> String {
        let n = |r: &Read<friends::View>| match r {
            Read::Got(v) => format!(
                "{}:{}:{}:{}",
                v.friends.len(),
                v.asks_in.len(),
                v.asks_out.len(),
                v.everyone().filter(|f| f.online).count()
            ),
            other => format!("{other:?}").chars().take(40).collect(),
        };
        format!(
            "{}|{:?}|{:?}|{:?}|{:?}|{}|{}|{:?}|{}",
            n(&self.friends),
            self.face,
            self.hours,
            self.rating,
            self.stats,
            self.act.as_ref().map_or("", |a| a.op.name()),
            self.line,
            self.confirm,
            self.pic.is_some(),
        )
    }

    pub fn friends_state(&self) -> &'static str {
        match &self.friends {
            Read::Asking => "asking",
            Read::None => "none",
            Read::Unreachable(_) => "unreachable",
            Read::Got(_) => "got",
        }
    }

    pub fn friends_count(&self) -> u32 {
        match &self.friends {
            Read::Got(v) => v.friends.len() as u32,
            _ => 0,
        }
    }

    pub fn profile_state(&self) -> &'static str {
        match (&self.hours, &self.rating, &self.stats) {
            (Read::Asking, _, _) | (_, Read::Asking, _) | (_, _, Read::Asking) => "asking",
            _ => "read",
        }
    }

    /// The face's name, once read.
    pub fn name(&self) -> Option<String> {
        match &self.face {
            Read::Got(f) => f.name.clone(),
            _ => None,
        }
    }
}

fn get(url: String, cap: usize) -> Slot {
    crate::platform::fetch(Req::get(url).cap(cap))
}

/// The reads, as the pages and the player's id ask for them, and a change's steps.
pub fn reads(mut front: ResMut<Front>, mut images: ResMut<Assets<Image>>) {
    let now = now_s();
    let id = front.who.id.clone();
    let nav = front.nav;
    let up = front.up;
    let e = &mut front.elo;
    if e.for_id != id {
        *e = EloState { for_id: id.clone(), ..EloState::default() };
    }
    let Some(id) = id else { return };
    // The face, once: the chip's name and picture.
    if e.face == Read::Asking && e.face_req.is_none() {
        e.face_req = Some(get(face::url(&id), 64 * 1024));
    }
    if let Some(a) = e.face_req.as_ref().and_then(Slot::take) {
        e.face_req = None;
        e.face = match a.and_then(|b| face::parse(&b)) {
            Ok(f) => {
                if let Some(p) = &f.picture {
                    e.pic_req = Some(get(face::picture_url(p), 2 << 20));
                }
                Read::Got(f)
            }
            Err(why) => Read::Unreachable(why),
        };
    }
    if let Some(a) = e.pic_req.as_ref().and_then(Slot::take) {
        e.pic_req = None;
        if let Ok(bytes) = a
            && let Ok(img) = Image::from_buffer(
                &bytes,
                ImageType::Extension("png"),
                CompressedImageFormats::NONE,
                true,
                ImageSampler::linear(),
                RenderAssetUsages::RENDER_WORLD,
            )
        {
            e.pic = Some(images.add(img));
        }
    }
    // FRIENDS while it's open.
    if up && nav == Nav::Friends && e.friends_req.is_none() && now - e.friends_at > FRIENDS_EVERY {
        e.friends_at = now;
        e.friends_req = Some(get(elopros::api(&format!("/friends/{id}")), friends::MAX_VIEW_BYTES));
    }
    if let Some(a) = e.friends_req.as_ref().and_then(Slot::take) {
        e.friends_req = None;
        e.friends = match a.and_then(|b| friends::parse(&b)) {
            Ok(v) => Read::Got(v),
            Err(why) => Read::Unreachable(why),
        };
    }
    // PROFILE while it's open.
    if up && nav == Nav::Profile && e.profile_reqs.is_none() && now - e.profile_at > PROFILE_EVERY {
        e.profile_at = now;
        let wallet = elopros::wallet(&id);
        e.profile_reqs = Some([
            match &wallet {
                Some(w) => get(profile::playtime_url(w), 64 * 1024),
                // Hours are counted for wallets.
                None => {
                    let s = Slot::default();
                    s.put(Ok(br#"{"games":[]}"#.to_vec()));
                    s
                }
            },
            get(profile::rating_url(&id), 64 * 1024),
            get(profile::stats_url(&id), 256 * 1024),
        ]);
    }
    if let Some(reqs) = &e.profile_reqs {
        let mut pending = 0;
        let got: Vec<Option<Result<Vec<u8>, String>>> = reqs.iter().map(Slot::take).collect();
        for (k, a) in got.into_iter().enumerate() {
            let Some(a) = a else {
                if match k {
                    0 => e.hours == Read::Asking,
                    1 => e.rating == Read::Asking,
                    _ => e.stats == Read::Asking,
                } {
                    pending += 1;
                }
                continue;
            };
            match (k, a) {
                (0, Ok(b)) => e.hours = profile::hours(&b),
                (1, Ok(b)) => e.rating = profile::rating(&b),
                (_, Ok(b)) => e.stats = profile::stats(&b),
                // A game with nothing declared, or a player never reported, answers 404.
                (0, Err(w)) => e.hours = not_found_is_none(w),
                (1, Err(w)) => e.rating = not_found_is_none(w),
                (_, Err(w)) => e.stats = not_found_is_none(w),
            }
        }
        if pending == 0 {
            e.profile_reqs = None;
        }
    }
    step_act(e, &id, now);
}

fn not_found_is_none<T>(why: String) -> Read<T> {
    if why.starts_with("404") || why == "HTTP 404" { Read::None } else { Read::Unreachable(why) }
}

/// A change's next step, when the last one's answer is in.
fn step_act(e: &mut EloState, me: &str, now: f64) {
    let Some(act) = e.act.as_mut() else { return };
    let (op, other) = (act.op, act.other.clone());
    type Next = Result<Option<(Stage, u64)>, String>;
    let next: Option<Next> = match &act.stage {
        Stage::Prepare(s) => s.take().map(|a| {
            let (text, at) = friends::check_offered(me, op, &other, now as u64, &a?)?;
            Ok(Some((Stage::Sign(crate::net::sign_text(text, me.to_string())), at)))
        }),
        Stage::Sign(s) => s.take().map(|a| {
            let sig = String::from_utf8(a?).map_err(|_| "the signature didn't read".to_string())?;
            let body = friends::act_body(me, op, &other, act.at, &sig);
            Ok(Some((
                Stage::Act(crate::platform::fetch(Req::post(elopros::api("/friends/act"), body))),
                act.at,
            )))
        }),
        Stage::Act(s) => s.take().map(|a| {
            let view = friends::parse_act(&a?)?;
            e.friends = Read::Got(view);
            Ok(None)
        }),
    };
    let Some(next) = next else { return };
    match next {
        Ok(Some((stage, at))) => {
            if let Some(act) = e.act.as_mut() {
                act.stage = stage;
                act.at = at;
            }
        }
        Ok(None) => {
            e.line = op.done().to_string();
            e.act = None;
        }
        Err(why) => {
            e.line = format!("Not changed: {why}");
            e.act = None;
        }
    }
}

/// A click on FRIENDS.
pub fn pick(front: &mut Front, p: FriendPick, controls: &mut crate::input::Controls) {
    let me = front.who.id.clone();
    let e = &mut front.elo;
    match p {
        FriendPick::Refresh => e.friends_at = f64::NEG_INFINITY,
        FriendPick::Party(w) => {
            controls.lobby.push(el_client_core::lobby::ToServer::PartyInvite { player: w });
            e.line = "Asked into your party (they must be on this server).".into();
        }
        FriendPick::Remove(w) if e.confirm.as_deref() != Some(w.as_str()) => {
            e.confirm = Some(w);
            e.line = "REMOVE again to take them off your list.".into();
        }
        FriendPick::Remove(w) => {
            e.confirm = None;
            start(e, me, Op::Remove, w);
        }
        FriendPick::Change(op, w) => start(e, me, op, w),
    }
}

fn start(e: &mut EloState, me: Option<String>, op: Op, other: String) {
    if e.act.is_some() {
        e.line = "One change at a time.".into();
        return;
    }
    let Some(me) = me.as_deref().and_then(elopros::wallet) else {
        e.line = "Changing friends takes a wallet: an Elo Pros login changes them on its profile.".into();
        return;
    };
    let body = friends::prepare_body(&me, op, &other);
    let slot = crate::platform::fetch(Req::post(elopros::api("/friends/prepare"), body).cap(8 * 1024));
    e.line = "Asking your wallet to sign...".into();
    e.act = Some(Act { op, other, stage: Stage::Prepare(slot), at: 0 });
}

fn row(c: &mut Commands) -> Entity {
    c.spawn(Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: px(8),
        padding: UiRect::vertical(px(4)),
        border: UiRect::bottom(px(1)),
        ..default()
    })
    .insert(BorderColor::all(RULE))
    .id()
}

fn small(c: &mut Commands, text: &str, pick: FriendPick) -> Entity {
    c.spawn((ui::button(text, 0.0, false), Pick::Friend(pick))).id()
}

pub fn friends_page(c: &mut Commands, front: &Front) -> Vec<Entity> {
    let e = &front.elo;
    let panel = c.spawn(ui::panel(PANEL_W)).id();
    title_and_blurb(
        c,
        panel,
        "FRIENDS",
        "Your Elo Pros friends: who's on, who's in a match, the asks waiting.",
    );
    let pane = c.spawn(ui::pane(true)).id();
    if front.who.id.is_none() {
        let l = c.spawn(label("Friends show here once you sign in.", 15.0, DIM)).id();
        c.entity(panel).add_child(l);
        return vec![panel];
    }
    let mut lines: Vec<Entity> = Vec::new();
    match &e.friends {
        Read::Got(v) => {
            if v.blind() {
                lines.push(c.spawn(label("Elo Pros can't see who's playing just now.", 14.0, FAINT)).id());
            }
            let on = v.everyone().filter(|f| f.online).count();
            lines.push(c.spawn(label(format!("{} friends, {on} on", v.friends.len()), 15.0, DIM)).id());
            if !v.asks_in.is_empty() {
                lines.push(c.spawn(ui::group_rule("ASKS")).id());
            }
            for a in &v.asks_in {
                let r = row(c);
                let who = a.name.clone().unwrap_or_else(|| friends::short(&a.wallet));
                let l = c
                    .spawn((
                        label(format!("{who} asks to be friends"), 15.0, TEXT),
                        Node { flex_grow: 1.0, ..default() },
                    ))
                    .id();
                let y = small(c, "ACCEPT", FriendPick::Change(Op::Accept, a.wallet.clone()));
                let n = small(c, "DECLINE", FriendPick::Change(Op::Decline, a.wallet.clone()));
                c.entity(r).add_children(&[l, y, n]);
                lines.push(r);
            }
            for a in &v.asks_out {
                let r = row(c);
                let who = a.name.clone().unwrap_or_else(|| friends::short(&a.wallet));
                let l = c
                    .spawn((label(format!("Asked {who}"), 15.0, DIM), Node { flex_grow: 1.0, ..default() }))
                    .id();
                let x = small(c, "CANCEL", FriendPick::Change(Op::Cancel, a.wallet.clone()));
                c.entity(r).add_children(&[l, x]);
                lines.push(r);
            }
        }
        other => {
            let l = profile::state_line(other, "your friends").unwrap_or_default();
            lines.push(
                c.spawn(label(l, 15.0, if matches!(other, Read::Unreachable(_)) { BAD } else { DIM })).id(),
            );
        }
    }
    if !e.line.is_empty() {
        lines.push(c.spawn(label(e.line.clone(), 14.0, GOLD)).id());
    }
    let gap = c.spawn(ui::spacer()).id();
    let refresh = c.spawn((ui::button("REFRESH", 0.0, false), Pick::Friend(FriendPick::Refresh))).id();
    let open = c.spawn((ui::button("ADD FRIENDS ON ELO PROS", 0.0, false), Pick::Profile)).id();
    c.entity(panel).add_children(&lines).add_children(&[gap, refresh, open]);
    // The list.
    if let Read::Got(v) = &e.friends {
        let now = now_s() as u64;
        let rows = v.ordered();
        if rows.is_empty() {
            let l = c.spawn(label("No friends yet: ask someone from your Elo Pros profile.", 17.0, DIM)).id();
            c.entity(pane).add_child(l);
        }
        for f in rows.iter().take(ROWS) {
            let r = row(c);
            let col = c
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    row_gap: px(2),
                    ..default()
                })
                .id();
            let name = c.spawn(strong(f.label(), 18.0, if f.online { TITLE } else { DIM })).id();
            let line = friends::presence_line(f, now);
            let l = c.spawn(label(line, 14.0, if f.on_eloram().is_some() { GOLD } else { FAINT })).id();
            c.entity(col).add_children(&[name, l]);
            c.entity(r).add_child(col);
            if f.on_eloram().is_some() {
                let p = small(c, "INVITE TO PARTY", FriendPick::Party(f.wallet.clone()));
                c.entity(r).add_child(p);
            }
            if f.buddy.is_none() {
                let sure = e.confirm.as_deref() == Some(f.wallet.as_str());
                let x = small(c, if sure { "SURE?" } else { "REMOVE" }, FriendPick::Remove(f.wallet.clone()));
                c.entity(r).add_child(x);
            }
            c.entity(pane).add_child(r);
        }
        if rows.len() > ROWS {
            let l =
                c.spawn(label(format!("and {} more on your profile", rows.len() - ROWS), 14.0, FAINT)).id();
            c.entity(pane).add_child(l);
        }
    }
    vec![panel, pane]
}

pub fn profile_page(c: &mut Commands, front: &Front) -> Vec<Entity> {
    let e = &front.elo;
    let panel = c.spawn(ui::panel(PANEL_W)).id();
    title_and_blurb(
        c,
        panel,
        "PROFILE",
        "You on Elo Pros: your face, your hours, your rating and what you've done here.",
    );
    if front.who.id.is_none() {
        let l = c.spawn(label("Sign in to see this: a guest has no profile.", 15.0, DIM)).id();
        c.entity(panel).add_child(l);
        return vec![panel];
    }
    // The face.
    let face_row = row(c);
    let pic = match &e.pic {
        Some(h) => {
            c.spawn((ImageNode::new(h.clone()), Node { width: px(72), height: px(72), ..default() })).id()
        }
        None => c
            .spawn((
                Node {
                    width: px(72),
                    height: px(72),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(ui::ROW_HOVER),
                children![strong(
                    front.who.shown_name().chars().next().unwrap_or('?').to_ascii_uppercase().to_string(),
                    30.0,
                    GOLD
                )],
            ))
            .id(),
    };
    let col = c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(4), ..default() }).id();
    let n = c.spawn(strong(front.who.shown_name(), 24.0, TITLE)).id();
    let l = c.spawn(label(front.who.line(), 14.0, DIM)).id();
    c.entity(col).add_children(&[n, l]);
    c.entity(face_row).add_children(&[pic, col]);
    let mut lines = vec![face_row];
    let mut say =
        |c: &mut Commands, text: String, color: Color| lines.push(c.spawn(label(text, 16.0, color)).id());
    match &e.hours {
        Read::Got(h) => say(c, h.line(), TEXT),
        other => say(c, profile::state_line(other, "your hours").unwrap_or_default(), FAINT),
    }
    match &e.rating {
        Read::Got(r) => say(c, r.line(), GOLD),
        Read::None => say(c, "Not rated yet: play RANKED to place.".into(), FAINT),
        other => say(c, profile::state_line(other, "your rating").unwrap_or_default(), FAINT),
    }
    let gap = c.spawn(ui::spacer()).id();
    let open = c.spawn((ui::button("OPEN ON ELO PROS", 0.0, false), Pick::Profile)).id();
    c.entity(panel).add_children(&lines).add_children(&[gap, open]);
    // Stats, achievements, boards.
    let pane = c.spawn(ui::pane(true)).id();
    match &e.stats {
        Read::Got(s) => {
            let rule = c.spawn(ui::group_rule("IN ELORAM")).id();
            c.entity(pane).add_child(rule);
            for (name, v) in &s.stats {
                let l = c.spawn(label(format!("{name}: {v}"), 16.0, TEXT)).id();
                c.entity(pane).add_child(l);
            }
            let rule = c.spawn(ui::group_rule("ACHIEVEMENTS")).id();
            let l = c.spawn(label(format!("{} of {} earned", s.earned, s.total), 16.0, TEXT)).id();
            c.entity(pane).add_children(&[rule, l]);
            for a in &s.recent {
                let l = c.spawn(label(a.clone(), 15.0, GOLD)).id();
                c.entity(pane).add_child(l);
            }
            if !s.boards.is_empty() {
                let rule = c.spawn(ui::group_rule("BOARDS")).id();
                c.entity(pane).add_child(rule);
                for (name, score, rank, format) in &s.boards {
                    let rank = rank.map(|r| format!(", #{r}")).unwrap_or_default();
                    let l = c
                        .spawn(label(
                            format!("{name}: {}{rank}", profile::score_text(*score, format)),
                            15.0,
                            TEXT,
                        ))
                        .id();
                    c.entity(pane).add_child(l);
                }
            }
        }
        other => {
            let l = c
                .spawn(label(profile::state_line(other, "your matches").unwrap_or_default(), 17.0, DIM))
                .id();
            c.entity(pane).add_child(l);
        }
    }
    vec![panel, pane]
}

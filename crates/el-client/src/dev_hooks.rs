//! `window.__el`: a small status object the Playwright tests (and curious humans) can read.

use bevy::prelude::*;
use wasm_bindgen::{JsCast, JsValue};

/// Fields published to `window.__el` a few times a second (every frame on a slow page). Kept as
/// Rust values and made JavaScript's only when published: setting one is a store, not a trip
/// across to JavaScript, however many a frame sets.
#[derive(Resource, Default)]
pub struct DevStatus {
    pub fields: Vec<(&'static str, Field)>,
}

/// A field's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Num(f64),
    Bool(bool),
    Text(String),
}

macro_rules! field_from_num {
    ($($t:ty),*) => {$(
        impl From<$t> for Field {
            fn from(v: $t) -> Field {
                Field::Num(v as f64)
            }
        }
    )*};
}
field_from_num!(u8, u16, u32, u64, usize, i32, i64, f32, f64);

impl From<bool> for Field {
    fn from(v: bool) -> Field {
        Field::Bool(v)
    }
}

impl From<&str> for Field {
    fn from(v: &str) -> Field {
        Field::Text(v.to_string())
    }
}

impl From<String> for Field {
    fn from(v: String) -> Field {
        Field::Text(v)
    }
}

impl From<&Field> for JsValue {
    fn from(f: &Field) -> JsValue {
        match f {
            Field::Num(n) => JsValue::from_f64(*n),
            Field::Bool(b) => JsValue::from_bool(*b),
            Field::Text(t) => JsValue::from_str(t),
        }
    }
}

impl DevStatus {
    pub fn set(&mut self, key: &'static str, value: impl Into<Field>) {
        let value = value.into();
        match self.fields.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.fields.push((key, value)),
        }
    }
}

pub struct DevHooksPlugin;

impl Plugin for DevHooksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DevStatus>().add_systems(Last, publish);
    }
}

/// Below this frame rate every frame publishes: frames come in bunches on a page drawing in
/// software (a quick one, then one seconds long, even a minute with a terminal open over the city),
/// and a frame skipped for coming soon after the last would leave a test reading a state the game
/// has left (a terminal Esc has closed) for as long as the next one takes.
const SLOW_FPS: f32 = 8.0;

fn publish(
    time: Res<Time<Real>>,
    mut last: Local<f32>,
    mut frames: Local<u32>,
    mut fps_window: Local<(f32, u32)>,
    mut slow: Local<bool>,
    mut status: ResMut<DevStatus>,
) {
    *frames += 1;
    let now = time.elapsed_secs();
    if *frames == 1 {
        hide_boot_overlay();
    }
    if now - fps_window.0 >= 2.0 {
        let fps = (*frames - fps_window.1) as f32 / (now - fps_window.0);
        *fps_window = (now, *frames);
        *slow = fps < SLOW_FPS;
        status.set("fps", fps);
    }
    if now - *last < 0.25 && !*slow {
        return;
    }
    *last = now;
    let frames = *frames;
    status.set("frames", frames);
    let Some(window) = web_sys::window() else { return };
    let obj = js_sys::Object::new();
    for (k, v) in &status.fields {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &JsValue::from(v));
    }
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__el"), &obj);
}

/// Fades out the HTML "loading" overlay once Bevy renders its first frame (the page's
/// `elHideBoot`, which hides it once it has faded; or at once, without it).
fn hide_boot_overlay() {
    let Some(window) = web_sys::window() else { return };
    let fade = js_sys::Reflect::get(&window, &JsValue::from_str("elHideBoot"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
    if let Some(f) = fade {
        let _ = f.call0(&window);
    } else if let Some(el) = window.document().and_then(|d| d.get_element_by_id("boot")) {
        let _ = el.class_list().add_1("hidden");
    }
}

/// What a test (or the console) can tell the champion through `window.__elInbox`: an array of
/// commands, read and emptied every frame, each becoming the order the mouse or keys would give.
#[derive(serde::Deserialize, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Cmd {
    Move {
        x: f32,
        y: f32,
    },
    AttackMove {
        x: f32,
        y: f32,
    },
    Attack {
        slot: u8,
    },
    Cast {
        slot: u8,
        x: Option<f32>,
        y: Option<f32>,
        unit: Option<u8>,
    },
    Stop,
    Hold,
    Level {
        slot: u8,
    },
    Pick {
        card: u8,
    },
    Reroll {
        card: u8,
    },
    Buy {
        item: u8,
    },
    Sell {
        inv: u8,
    },
    Shop {
        open: bool,
    },
    Lock {
        on: bool,
    },
    /// Champion select: a reroll, a champion off the bench, ready.
    ChampReroll,
    ChampSwap {
        champ: u8,
    },
    Ready,
    /// An ability's indicator shown, as if its key were held (`slot: null` lets go without casting).
    Aim {
        slot: Option<u8>,
    },
    /// A ping (`el_proto::order::ping`; 0, look here, by default), at a place, naming a unit.
    Ping {
        x: f32,
        y: f32,
        kind: Option<u8>,
        unit: Option<u8>,
    },
    /// A vote to surrender (a yes opens one).
    Surrender {
        yes: bool,
    },
    /// The link closed under the page, as a lost connection would (it dials again).
    DropLink,
    /// The camera freed and put over a point of the map (`zoom` as the wheel's, 0.7 to 1.4).
    Camera {
        x: f32,
        y: f32,
        zoom: Option<f32>,
    },
    /// The fog's darkness drawn or not (only the picture: what the team isn't sent still isn't).
    Fog {
        on: bool,
    },
    /// A volume on the page's audio panel: `"master"` or a bus's name (`effects`, `voice`, `ui`,
    /// `ambience`, `music`), 0 to 1.
    Volume {
        bus: String,
        v: f32,
    },
    Mute {
        on: bool,
    },
}

pub struct InboxPlugin;

impl Plugin for InboxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                inbox.after(crate::net::NetSet::Receive).before(crate::net::NetSet::Send),
                match_status.after(crate::net::NetSet::Send),
            ),
        );
    }
}

fn inbox(
    net: NonSend<crate::net::NetState>,
    mut controls: ResMut<crate::input::Controls>,
    mut rig: ResMut<crate::camera::Rig>,
    mut look: ResMut<crate::look::Look>,
    mut audio: ResMut<crate::audio::AudioSettings>,
    drawn: Res<crate::view::Drawn>,
) {
    use el_client_core::lobby::ToServer;
    use el_proto::{Aim, Order, UnitRef};
    let Some(window) = web_sys::window() else { return };
    let Ok(arr) = js_sys::Reflect::get(&window, &JsValue::from_str("__elInbox")) else { return };
    let Ok(arr) = arr.dyn_into::<js_sys::Array>() else { return };
    let unit = |slot: u8| UnitRef {
        slot,
        generation: drawn.units.get(usize::from(slot)).copied().flatten().map_or(0, |d| d.generation),
    };
    while arr.length() > 0 {
        let v = arr.shift();
        let text = js_sys::JSON::stringify(&v).ok().and_then(|s| s.as_string()).unwrap_or_default();
        let cmd = match serde_json::from_str::<Cmd>(&text) {
            Ok(c) => c,
            Err(e) => {
                web_sys::console::warn_1(&format!("__elInbox: {e}: {text}").into());
                continue;
            }
        };
        let order = match cmd {
            Cmd::Move { x, y } => Order::Move { to: Vec2::new(x, y) },
            Cmd::AttackMove { x, y } => Order::AttackMove { to: Vec2::new(x, y) },
            Cmd::Attack { slot } => Order::Attack { unit: unit(slot) },
            Cmd::Cast { slot, x, y, unit: u } => {
                let aim = match (x.zip(y), u) {
                    (Some((x, y)), _) => Aim::Point(Vec2::new(x, y)),
                    (None, Some(s)) => Aim::Unit(unit(s)),
                    (None, None) => Aim::None,
                };
                Order::Cast { slot, aim }
            }
            Cmd::Stop => Order::Stop,
            Cmd::Hold => Order::Hold,
            Cmd::Level { slot } => Order::LevelSkill { slot },
            Cmd::Pick { card } => Order::PickAugment { card },
            Cmd::Reroll { card } => Order::Reroll { card },
            Cmd::Buy { item } => Order::Buy { item },
            Cmd::Sell { inv } => Order::Sell { inv },
            Cmd::Ping { x, y, kind, unit: u } => {
                Order::Ping { kind: kind.unwrap_or(0), at: Vec2::new(x, y), unit: u.map(unit) }
            }
            Cmd::Surrender { yes } => Order::Surrender { yes },
            Cmd::DropLink => {
                if let Some(t) = net.get() {
                    t.close();
                }
                continue;
            }
            Cmd::Shop { open } => {
                controls.shop_open = open;
                continue;
            }
            Cmd::Lock { on } => {
                rig.locked = on;
                continue;
            }
            Cmd::Camera { x, y, zoom } => {
                rig.locked = false;
                rig.placed = true;
                rig.focus = Vec2::new(x, y);
                rig.zoom = zoom.unwrap_or(rig.zoom).clamp(0.7, 1.4);
                continue;
            }
            Cmd::Fog { on } => {
                look.fog = on;
                continue;
            }
            Cmd::Volume { bus, v } => {
                audio.set(&bus, v);
                audio.save();
                continue;
            }
            Cmd::Mute { on } => {
                audio.muted = on;
                audio.save();
                continue;
            }
            Cmd::ChampReroll => {
                controls.lobby.push(ToServer::Reroll);
                continue;
            }
            Cmd::ChampSwap { champ } => {
                controls.lobby.push(ToServer::Swap { champ });
                continue;
            }
            Cmd::Ready => {
                controls.lobby.push(ToServer::Ready);
                continue;
            }
            Cmd::Aim { slot } => {
                controls.aiming = slot.filter(|&s| s < 6);
                continue;
            }
        };
        controls.pending.push(order);
    }
}

/// The match, for `window.__el`.
fn match_status(
    session: Res<crate::session::Session>,
    controls: Res<crate::input::Controls>,
    drawn: Res<crate::view::Drawn>,
    mut status: ResMut<DevStatus>,
    mut walk_max: Local<f32>,
    mut corrections: Local<(u64, u32)>,
) {
    if let Some(w) = session.watch.as_deref() {
        status.set("watch_tick", w.referee.m.tick);
        status.set("watch_available", w.available);
        status.set("watch_hashes", w.referee.checked);
        status.set("watch_from", w.from);
        status.set("watch_live", w.live);
        status.set("watch_speed", w.speed);
        status.set("watch_units", drawn.units.iter().filter(|d| d.is_some()).count() as u32);
        if let Some(e) = &w.error {
            status.set("watch_error", e.as_str());
        }
        return;
    }
    if let Some(sel) = &session.select {
        let names = |cs: &[u8]| -> String {
            cs.iter()
                .filter_map(|&c| el_sim::content::champions::CHAMPIONS.get(usize::from(c)))
                .map(|d| d.name)
                .collect::<Vec<_>>()
                .join(",")
        };
        let ids = |cs: &[u8]| cs.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",");
        status.set(
            "select_champion",
            sel.mine().map_or("", |c| el_sim::content::champions::CHAMPIONS[usize::from(c)].name),
        );
        status.set("select_champion_id", sel.mine().map_or(-1, i32::from));
        status.set("select_bench", names(&sel.bench));
        status.set("select_bench_ids", ids(&sel.bench));
        status.set("select_rerolls", u32::from(sel.rerolls));
        status.set("select_ready", sel.ready);
        status.set("select_team", names(&sel.picks));
    }
    let Some(c) = session.client.as_deref() else { return };
    let s = c.seat;
    let u = &c.pred.units[s];
    let ch = &c.pred.champs[s];
    if let Some(k) = c.correction
        && corrections.1 != k.tick
    {
        corrections.1 = k.tick;
        corrections.0 += 1;
        if k.cause == el_client_core::Cause::Walking {
            *walk_max = walk_max.max(k.error);
        }
    }
    let seen = |enemy: bool, champs: bool| {
        drawn
            .units
            .iter()
            .enumerate()
            .filter(|(i, d)| {
                d.is_some_and(|d| (d.team != c.team) == enemy && (!champs || *i < el_sim::units::SEATS))
            })
            .count() as u32
    };
    let items: Vec<String> =
        ch.items.iter().filter(|&&i| i != el_sim::content::items::NONE).map(|i| i.to_string()).collect();
    let st = &c.stats;
    status.set("seat", s as u32);
    status.set("team", u32::from(c.team));
    status.set("champion", ch.def().name);
    status.set("headliner", c.pred.headliner().name);
    status.set("aiming", controls.aiming.map_or(-1, i32::from));
    status.set("keys", if controls.wasd { "wasd" } else { "league" });
    status.set("steer", u32::from(controls.steer));
    status.set("started", c.started());
    status.set("tick", c.world.tick);
    status.set("pred_tick", c.pred.tick);
    status.set("x", u.pos.x);
    status.set("y", u.pos.y);
    status.set("alive", u.alive);
    status.set("hp", u.hp);
    status.set("max_hp", u.stats.max_hp);
    status.set("level", u32::from(ch.level));
    status.set("gold", ch.gold);
    status.set("items", items.join(","));
    status.set("augments", u32::from(ch.n_aug));
    status.set("screen_open", ch.screen.open);
    status.set("in_shop", c.pred.in_shop(s));
    status.set("points", u32::from(ch.points));
    status.set("enemies", seen(true, false));
    status.set("enemy_champions", seen(true, true));
    // Enemies that move (champions and minions): what fog hides.
    let mobile = drawn
        .units
        .iter()
        .enumerate()
        .filter(|(i, d)| {
            d.is_some_and(|d| d.team != c.team)
                && !(el_sim::units::SEATS..el_sim::units::FIRST_MINION).contains(i)
        })
        .count() as u32;
    status.set("enemy_mobile", mobile);
    status.set("ranks", ch.ranks.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(","));
    status.set(
        "cds",
        ch.ready.iter().map(|r| r.saturating_sub(c.pred.tick).to_string()).collect::<Vec<_>>().join(","),
    );
    status.set("allies", seen(false, false));
    status.set("missiles", c.missiles.live().count() as u32);
    status.set("datagrams", st.datagrams as f64);
    status.set("stale", st.stale as f64);
    status.set("errors", st.errors as f64);
    status.set("records", st.records as f64);
    status.set("depth", u32::from(st.depth));
    status.set("missed", u32::from(st.missed));
    status.set("match_rtt_ms", c.clock.rtt * 1_000.0);
    status.set("lead", c.clock.lead);
    status.set("packets", c.stats.packets as f64);
    status.set("orders_given", c.orders.given as f64);
    status.set("orders_unacked", c.orders.outstanding() as u32);
    status.set("corrections", corrections.0 as f64);
    status.set("walk_err_max", *walk_max);
    let vote = &c.world.surrender[usize::from(c.team)];
    status.set("vote_open", vote.open());
    status.set("vote_yes", vote.yes.count_ones());
    status.set("pings_heard", session.pings);
    status.set("kills", u32::from(session.kd[s].0));
    status.set("deaths", u32::from(session.kd[s].1));
    if let Some(w) = session.winner {
        status.set("winner", u32::from(w));
    }
}

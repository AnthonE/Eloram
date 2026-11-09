//! The menu (Esc, or the button at the top left): every key, the settings and the sound, over the
//! world, which goes on under it. Gates' shape (`AnthonE/Gates`, `render/pause.rs` and
//! `settings.rs`): a rail of pages down the left, the way back into the game at its foot, a page of
//! rows on the right, each a label and its choices, and only the settings that do something here.
//!
//! - **Controls**: the keys' layout (League's default or its WASD), casting (quick, or with an
//!   indicator), the camera (free or locked on the champion), and under them every key of the
//!   layout chosen, from `el_client_core::controls`, the same table the HUD names them from.
//! - **Graphics**: the picture's tier, which the renderer is built with, so it takes effect when the
//!   page loads again (a reload keeps the seat in a match), and what's drawing now.
//! - **Sound**: the master's volume and each bus's, and mute (the page's own panel until now).
//!
//! Esc is one thing at a time, the top one first: an aimed cast (or an armed attack-move) is called
//! off (`input.rs`), then the shop is closed, and only then is the menu opened, letting go of the
//! unit picked; with the menu up, Esc closes it. While it's up, the keys are the menu's alone (the
//! champion goes on with its last order: the world doesn't stop for it, as a MOBA's doesn't), and
//! neither the map nor the minimap takes a click. Each change is kept for the next visit
//! (`config::remember`).

use bevy::input::InputSystems;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use el_client_core::controls::{self, Group, Layout};
use el_client_core::fx::sound::Bus;

use crate::audio::AudioSettings;
use crate::config::Prefs;
use crate::dev_hooks::{DevStatus, Field};
use crate::hud::{
    BUTTON, BUTTON_GOLD, DIM, FRAME, FRAME_SOFT, GOLD, TEXT, TRIM, Tint, framed, outline, text,
};
use crate::input::Controls;
use crate::session::{Phase, Session};

/// The menu's state.
#[derive(Resource, Default)]
pub struct Menu {
    pub open: bool,
    pub tab: Tab,
    /// The volume's slider held (0 the master's, then each bus's).
    drag: Option<usize>,
    /// LEAVE MATCH pressed once: a second press leaves.
    leave_armed: bool,
}

impl Menu {
    pub fn show(&mut self, open: bool) {
        self.open = open;
        if !open {
            self.drag = None;
            self.leave_armed = false;
        }
    }
}

/// A page of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Controls,
    Graphics,
    Sound,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Controls, Tab::Graphics, Tab::Sound];

    /// Its name on the rail; lowercase, its name to a test (`{t:"menu",tab}`, `window.__el`).
    pub fn title(self) -> &'static str {
        match self {
            Tab::Controls => "CONTROLS",
            Tab::Graphics => "GRAPHICS",
            Tab::Sound => "SOUND",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Tab::ALL.into_iter().find(|t| t.title().eq_ignore_ascii_case(s))
    }
}

/// The button at the top left (the fx stage hides it: `stage.rs`).
#[derive(Component)]
pub struct TopButton;

/// What a button of the menu does.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Do {
    /// The button at the top left: opens the menu, or closes it.
    Toggle,
    Resume,
    Tab(Tab),
    Layout(Layout),
    Indicators(bool),
    Locked(bool),
    Low(bool),
    Reload,
    Mute,
    /// Out of the match (or practice) for good, back to the main menu: two presses.
    Leave,
}

/// The menu: the world dimmed, the panel in the middle.
#[derive(Component)]
struct Root;

/// A page, shown while its tab is.
#[derive(Component)]
struct Page(Tab);

/// The Controls page's keys, written again when the layout or the casting changes.
#[derive(Component)]
struct Sheet;

/// A volume's slider (0 the master's, then each bus's in `Bus::ALL`'s order): where a click
/// lands, and its fill.
#[derive(Component)]
struct Slider(usize);

#[derive(Component)]
struct SliderFill(usize);

/// Words the menu writes.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Words {
    Percent(usize),
    Mute,
    /// What draws now: the API, the tier, the frame rate.
    Now,
    /// LEAVE MATCH, or its second press's ask.
    Leave,
}

/// The LEAVE MATCH button, shown while there's a match to leave.
#[derive(Component)]
struct LeaveButton;

/// The note and the button that say the tier changes at a reload, while it would.
#[derive(Component)]
struct Pending;

/// The volumes' rows: the master's, then each bus's.
const VOLUMES: [&str; 6] = ["Master", "Effects", "Announcer", "Interface", "Ambience", "Music"];

/// The panel's size, in the HUD's pixels (1600 by 900, scaled: `hud.rs`).
const W: f32 = 1_000.0;
const H: f32 = 620.0;
const RAIL: f32 = 200.0;

/// The colour of a choice not taken, and taken.
const OFF: Color = BUTTON;
const ON: Color = BUTTON_GOLD;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menu>()
            .add_systems(PostStartup, setup)
            .add_systems(PreUpdate, keys.after(InputSystems))
            .add_systems(Update, (press, slide, show, sheet).chain().after(crate::net::NetSet::Send));
    }
}

/// Esc, the top thing first; and while the menu's up, the keys and the wheel are its alone.
fn keys(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut scroll: ResMut<AccumulatedMouseScroll>,
    mut menu: ResMut<Menu>,
    mut controls: ResMut<Controls>,
    session: Res<Session>,
    front: Res<crate::front::Front>,
) {
    if keys.just_pressed(KeyCode::Escape) && front.up && !menu.open {
        // The main menu's own (`front`): back to PLAY, never the Esc menu over it.
    } else if keys.just_pressed(KeyCode::Escape) {
        // The shop as it's drawn (`hud.rs`): left open off the platform, it isn't there to close.
        let shop = controls.shop_open
            && session.phase == Phase::Playing
            && session.client.as_deref().is_some_and(|c| c.pred.in_shop(c.seat));
        if menu.open {
            menu.show(false);
            keys.clear_just_pressed(KeyCode::Escape);
        } else if controls.aiming.is_some() || controls.armed.is_some() {
            // `input.rs` calls it off.
        } else if shop {
            controls.shop_open = false;
            keys.clear_just_pressed(KeyCode::Escape);
        } else {
            menu.show(true);
            controls.selected = None;
            keys.clear_just_pressed(KeyCode::Escape);
        }
    }
    if menu.open {
        // Nothing held reaches the game (WASD's walk stops, Tab's scoreboard goes), and nothing
        // pressed does.
        keys.reset_all();
        scroll.delta = Vec2::ZERO;
    }
}

/// A row of a page: its label, then what's in it.
fn row(label: &str) -> impl Bundle {
    (
        Node { align_items: AlignItems::Center, column_gap: px(10), min_height: px(38), ..default() },
        children![(text(label, 12.0, DIM), Node { width: px(130), ..default() })],
    )
}

/// A button of the menu, without its words.
fn button_node(act: Do, width: Val, base: Color) -> impl Bundle {
    (
        Button,
        Node {
            width,
            height: px(32),
            padding: UiRect::horizontal(px(12)),
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
    )
}

/// A button of the menu.
fn button(act: Do, words: &str, width: Val, base: Color) -> impl Bundle {
    (button_node(act, width, base), children![(text(words, 13.0, TEXT), outline())])
}

/// One of a row's choices.
fn choice(act: Do, words: &str) -> impl Bundle {
    button(act, words, px(190), OFF)
}

#[allow(clippy::too_many_lines)]
fn setup(mut commands: Commands) {
    // The button at the top left (where the page's sound button was): three bars, as menus are.
    commands.spawn((
        Button,
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(10),
            width: px(40),
            height: px(34),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(4),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BorderColor::all(FRAME),
        BackgroundColor(OFF),
        Tint(OFF),
        Do::Toggle,
        TopButton,
        // Over the menu's dimming, so it closes it too.
        GlobalZIndex(45),
        children![bar(), bar(), bar()],
    ));

    // The rail: the pages, and the way back at its foot.
    let rail = commands
        .spawn((
            Node {
                width: px(RAIL),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(18)),
                row_gap: px(8),
                border: UiRect::right(px(1)),
                ..default()
            },
            BorderColor::all(FRAME_SOFT),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
        ))
        .with_children(|r| {
            r.spawn((text("ELORAM", 22.0, GOLD), outline()));
            r.spawn((text("MENU", 11.0, DIM), Node { margin: UiRect::bottom(px(14)), ..default() }));
            for t in Tab::ALL {
                r.spawn(button(Do::Tab(t), t.title(), percent(100), OFF));
            }
            r.spawn(Node { flex_grow: 1.0, ..default() });
            r.spawn((button_node(Do::Leave, percent(100), OFF), LeaveButton)).with_children(|b| {
                b.spawn((text("LEAVE MATCH", 14.0, TEXT), Words::Leave));
            });
            r.spawn(button(Do::Resume, "RESUME", percent(100), ON));
            r.spawn((
                text("Esc closes it", 11.0, DIM),
                TextLayout::justify(Justify::Center),
                Node { align_self: AlignSelf::Center, ..default() },
            ));
        })
        .id();

    // The pages.
    let controls_page = commands
        .spawn((
            Node { flex_direction: FlexDirection::Column, display: Display::None, ..default() },
            Page(Tab::Controls),
        ))
        .with_children(|p| {
            p.spawn(row("KEYS")).with_children(|r| {
                r.spawn(choice(Do::Layout(Layout::League), "LEAGUE'S DEFAULT"));
                r.spawn(choice(Do::Layout(Layout::Wasd), "WASD"));
            });
            p.spawn(row("CASTING")).with_children(|r| {
                r.spawn(choice(Do::Indicators(false), "QUICK CAST"));
                r.spawn(choice(Do::Indicators(true), "WITH INDICATOR"));
            });
            p.spawn(row("CAMERA")).with_children(|r| {
                r.spawn(choice(Do::Locked(false), "FREE"));
                r.spawn(choice(Do::Locked(true), "LOCKED ON YOU"));
            });
            p.spawn((
                Node { height: px(1), margin: UiRect::vertical(px(12)), ..default() },
                BackgroundColor(FRAME_SOFT),
            ));
            p.spawn((Node { column_gap: px(28), ..default() }, Sheet));
        })
        .id();

    let graphics_page = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                display: Display::None,
                ..default()
            },
            Page(Tab::Graphics),
        ))
        .with_children(|p| {
            p.spawn(row("QUALITY")).with_children(|r| {
                r.spawn(choice(Do::Low(true), "LOW"));
                r.spawn(choice(Do::Low(false), "HIGH"));
            });
            p.spawn((
                text(
                    "High draws shadows, smooths edges with multisampling, lights the braziers and, \
                     on WebGPU, adds HDR and bloom. Low draws none of them and reads the ground from \
                     fewer textures: for older machines.",
                    13.0,
                    DIM,
                ),
                Node { max_width: px(620), ..default() },
            ));
            p.spawn((text("", 13.0, TEXT), Words::Now));
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(10),
                    padding: UiRect::all(px(12)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(6)),
                    margin: UiRect::top(px(6)),
                    max_width: px(620),
                    display: Display::None,
                    ..default()
                },
                BorderColor::all(TRIM),
                BackgroundColor(Color::srgba(0.30, 0.24, 0.10, 0.25)),
                Pending,
            ))
            .with_children(|b| {
                b.spawn(text(
                    "This takes effect when the page loads again. A reload in a match keeps your seat.",
                    13.0,
                    GOLD,
                ));
                b.spawn(button(Do::Reload, "RELOAD NOW", px(190), ON));
            });
        })
        .id();

    let sound_page = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                display: Display::None,
                ..default()
            },
            Page(Tab::Sound),
        ))
        .with_children(|p| {
            for (k, name) in VOLUMES.iter().enumerate() {
                p.spawn(row(&name.to_uppercase())).with_children(|r| {
                    // Where a click lands is taller than the bar it moves.
                    r.spawn((
                        Node { width: px(380), height: px(26), align_items: AlignItems::Center, ..default() },
                        Interaction::None,
                        RelativeCursorPosition::default(),
                        Slider(k),
                    ))
                    .with_children(|hit| {
                        hit.spawn((
                            Node {
                                width: percent(100),
                                height: px(8),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(4)),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                            BackgroundColor(Color::srgb(0.05, 0.06, 0.08)),
                            children![(
                                Node { width: percent(0), height: percent(100), ..default() },
                                BackgroundColor(TRIM),
                                SliderFill(k),
                            )],
                        ));
                    });
                    r.spawn((text("", 13.0, TEXT), Node { width: px(48), ..default() }, Words::Percent(k)));
                });
            }
            p.spawn(row("")).with_children(|r| {
                r.spawn((
                    button_node(Do::Mute, px(190), OFF),
                    children![(text("", 13.0, TEXT), Words::Mute)],
                ));
            });
        })
        .id();

    let pane = commands
        .spawn(Node {
            flex_grow: 1.0,
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(24)),
            ..default()
        })
        .add_children(&[controls_page, graphics_page, sound_page])
        .id();
    let panel = commands
        .spawn((
            Node {
                width: px(W),
                height: px(H),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                overflow: Overflow::clip(),
                ..default()
            },
            framed(),
        ))
        .add_children(&[rail, pane])
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            // It takes the clicks that miss the panel: the map under it isn't the player's now.
            BackgroundColor(Color::srgba(0.01, 0.015, 0.03, 0.62)),
            Interaction::None,
            GlobalZIndex(40),
            Root,
        ))
        .add_child(panel);
}

fn bar() -> impl Bundle {
    (Node { width: px(18), height: px(2), ..default() }, BackgroundColor(TEXT))
}

/// A button of the menu pressed.
#[allow(clippy::too_many_arguments)]
fn press(
    q: Query<(&Interaction, &Do), Changed<Interaction>>,
    mut menu: ResMut<Menu>,
    mut prefs: ResMut<Prefs>,
    mut rig: ResMut<crate::camera::Rig>,
    mut audio: ResMut<AudioSettings>,
    mut controls: ResMut<Controls>,
    session: Res<Session>,
) {
    for (i, act) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        match *act {
            Do::Toggle => {
                let open = !menu.open;
                menu.show(open);
                if open {
                    controls.selected = None;
                }
            }
            Do::Resume => menu.show(false),
            Do::Tab(t) => menu.tab = t,
            Do::Layout(l) => {
                if prefs.layout != l {
                    prefs.set_layout(l);
                }
            }
            Do::Indicators(on) => {
                if prefs.indicators != on {
                    prefs.set_indicators(on);
                }
            }
            Do::Locked(on) => {
                if rig.locked != on {
                    rig.set_locked(on);
                }
            }
            Do::Low(low) => {
                if prefs.low_next != low {
                    prefs.set_low_next(low);
                }
            }
            Do::Reload => {
                crate::platform::reload();
            }
            Do::Mute => {
                audio.muted = !audio.muted;
                audio.save();
            }
            Do::Leave if !menu.leave_armed => menu.leave_armed = true,
            Do::Leave => {
                if in_match(session.phase) {
                    controls.lobby.push(el_client_core::lobby::ToServer::Leave);
                }
                menu.show(false);
            }
        }
    }
}

/// A volume's slider: pressed on, it follows the cursor while the button's held, and is kept
/// when it's let go.
fn slide(
    mouse: Res<ButtonInput<MouseButton>>,
    q: Query<(&Slider, &RelativeCursorPosition)>,
    mut menu: ResMut<Menu>,
    mut audio: ResMut<AudioSettings>,
) {
    if !menu.open || menu.tab != Tab::Sound {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        menu.drag = q.iter().find(|(_, r)| r.cursor_over()).map(|(s, _)| s.0);
    }
    let Some(k) = menu.drag else { return };
    if let Some(n) = q.iter().find(|(s, _)| s.0 == k).and_then(|(_, r)| r.normalized) {
        let v = ((n.x + 0.5) * 20.0).round().clamp(0.0, 20.0) / 20.0;
        let bus = if k == 0 { "master" } else { Bus::ALL[k - 1].name() };
        if volume(&audio, k) != v {
            audio.set(bus, v);
        }
    }
    if !mouse.pressed(MouseButton::Left) {
        menu.drag = None;
        audio.save();
    }
}

fn volume(audio: &AudioSettings, k: usize) -> f32 {
    if k == 0 { audio.master } else { audio.buses[k - 1] }
}

/// The menu shown or not, its page, each choice lit or not, the volumes, the words.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
/// While there's a seat to leave.
fn in_match(phase: Phase) -> bool {
    matches!(phase, Phase::Playing | Phase::Reconnecting | Phase::Selecting)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn show(
    menu: Res<Menu>,
    session: Res<Session>,
    prefs: Res<Prefs>,
    rig: Res<crate::camera::Rig>,
    audio: Res<AudioSettings>,
    mut status: ResMut<DevStatus>,
    mut nodes: Query<
        (&mut Node, Option<&Root>, Option<&Page>, Option<&Pending>, Option<&SliderFill>, Has<LeaveButton>),
        Or<(With<Root>, With<Page>, With<Pending>, With<SliderFill>, With<LeaveButton>)>,
    >,
    mut buttons: Query<(&Do, &Interaction, &mut Tint, &mut BackgroundColor, &mut BorderColor)>,
    mut words: Query<(&Words, &mut Text)>,
) {
    for (mut node, root, page, pending, fill, leave) in &mut nodes {
        let display = if leave {
            Some(in_match(session.phase))
        } else if root.is_some() {
            Some(menu.open)
        } else if let Some(p) = page {
            Some(p.0 == menu.tab)
        } else if pending.is_some() {
            Some(prefs.low_next != prefs.low)
        } else {
            None
        };
        if let Some(on) = display {
            let want = if on { Display::Flex } else { Display::None };
            if node.display != want {
                node.display = want;
            }
        }
        if let Some(f) = fill {
            let want = percent(volume(&audio, f.0) * 100.0);
            if node.width != want {
                node.width = want;
            }
        }
    }
    for (act, i, mut tint, mut bg, mut border) in &mut buttons {
        let lit = match *act {
            Do::Toggle => menu.open,
            Do::Tab(t) => t == menu.tab,
            Do::Layout(l) => prefs.layout == l,
            Do::Indicators(on) => prefs.indicators == on,
            Do::Locked(on) => rig.locked == on,
            Do::Low(low) => prefs.low_next == low,
            Do::Mute => audio.muted,
            Do::Leave => menu.leave_armed,
            Do::Resume | Do::Reload => continue,
        };
        let base = if lit { ON } else { OFF };
        if tint.0 != base {
            tint.0 = base;
            bg.0 = match i {
                Interaction::None => base,
                _ => crate::hud::shade(base, 0.07),
            };
        }
        let edge = if lit { TRIM } else { FRAME };
        if *border != BorderColor::all(edge) {
            *border = BorderColor::all(edge);
        }
    }
    // Its words are inside it: worked out only while it's open (the frame it opens included), not
    // formatted every frame for a menu nobody sees.
    if menu.open {
        words_now(&prefs, &audio, &status, menu.leave_armed, &mut words);
    }
    status.set("menu_open", menu.open);
    status.set("menu_tab", menu.tab.title().to_lowercase());
    status.set("indicators", prefs.indicators);
    status.set("quality", if prefs.low { "low" } else { "high" });
    status.set("quality_next", if prefs.low_next { "low" } else { "high" });
}

/// The menu's words that change: the volumes, the mute button's, and what's drawing now.
fn words_now(
    prefs: &Prefs,
    audio: &AudioSettings,
    status: &DevStatus,
    leave_armed: bool,
    words: &mut Query<(&Words, &mut Text)>,
) {
    let fps = status.fields.iter().find(|(k, _)| *k == "fps").and_then(|(_, v)| match v {
        Field::Num(n) => Some(*n),
        _ => None,
    });
    // The bench's results (`bench.rs`), while a bench runs or once it's done.
    let bench = status.fields.iter().find(|(k, _)| *k == "bench").and_then(|(_, v)| match v {
        Field::Text(t) => Some(t.clone()),
        _ => None,
    });
    for (w, mut t) in words.iter_mut() {
        let want = match *w {
            Words::Percent(k) => format!("{:.0}%", volume(audio, k) * 100.0),
            Words::Mute => if audio.muted { "UNMUTE" } else { "MUTE" }.to_string(),
            Words::Leave => if leave_armed { "AGAIN TO LEAVE" } else { "LEAVE MATCH" }.to_string(),
            Words::Now => {
                let api = if !crate::platform::DESKTOP && crate::look::WEBGPU {
                    "WebGPU"
                } else if crate::look::WEBGPU {
                    "Vulkan or DirectX 12"
                } else {
                    "WebGL 2"
                };
                let tier = if prefs.low { "low" } else { "high" };
                let now = match fps.map(f64::round) {
                    Some(1.0) => format!("Drawing now: {api}, {tier}, a frame a second"),
                    Some(f) => format!("Drawing now: {api}, {tier}, {f:.0} frames a second"),
                    None => format!("Drawing now: {api}, {tier}"),
                };
                match &bench {
                    Some(b) => format!("{now}\nBench: {b}"),
                    None => now,
                }
            }
        };
        if **t != want {
            **t = want;
        }
    }
}

/// The Controls page's keys, two columns of groups, written again when the layout or the casting
/// changes.
fn sheet(
    mut commands: Commands,
    prefs: Res<Prefs>,
    q: Query<Entity, With<Sheet>>,
    mut drawn: Local<Option<(Layout, bool)>>,
) {
    let want = (prefs.layout, prefs.indicators);
    if *drawn == Some(want) {
        return;
    }
    let Ok(e) = q.single() else { return };
    *drawn = Some(want);
    let lines = controls::sheet(want.0, want.1);
    let mut sheet = commands.entity(e);
    sheet.despawn_children();
    // Two columns about as long: moving, the abilities and the team; the items and the camera.
    let columns: [&[Group]; 2] =
        [&[Group::Moving, Group::Abilities, Group::Team], &[Group::Items, Group::Camera]];
    sheet.with_children(|s| {
        for groups in columns {
            s.spawn(Node {
                flex_direction: FlexDirection::Column,
                width: px(370),
                row_gap: px(5),
                ..default()
            })
            .with_children(|col| {
                for &g in groups {
                    col.spawn((
                        text(g.title(), 11.0, GOLD),
                        Node { margin: UiRect::new(px(0), px(0), px(8), px(2)), ..default() },
                    ));
                    for line in lines.iter().filter(|l| l.group == g) {
                        // Each a line (`controls::KEYS_CHARS`, `DOES_CHARS`: the font's 7.2 px a
                        // character at 12 px), unwrapped: words that may wrap are given the height
                        // of their narrowest wrapping, a word a line.
                        col.spawn(Node { column_gap: px(10), ..default() }).with_children(|r| {
                            r.spawn((
                                text(line.keys, 12.0, TEXT),
                                TextLayout::no_wrap(),
                                Node { width: px(150), flex_shrink: 0.0, ..default() },
                            ));
                            r.spawn((
                                text(line.does, 12.0, DIM),
                                TextLayout::no_wrap(),
                                Node { width: px(212), flex_shrink: 0.0, ..default() },
                            ));
                        });
                    }
                }
            });
        }
    });
}

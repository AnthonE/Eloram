//! REPORT A BUG: F8 anywhere, or the button in the main menu's footer and the Esc menu. The frame
//! is shot first (the world, not the panel over it), then a panel asks what kind of trouble, a
//! title and a few lines; SEND (Ctrl+Enter) files it with Elo Pros' intake
//! (`el_client_core::elopros::bugs`), and the answer's id is shown. While it's open it takes the
//! keyboard: nothing typed reaches the game. On the desktop, a crash note the last run left
//! (`desktop.rs`'s panic hook) is offered once, to send or put away.

use std::sync::{Arc, Mutex};

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use el_client_core::elopros::bugs::{self, Kind, Report};

use super::ui::{self, BAD, FAINT, RULE, TITLE, label, strong};
use crate::hud::{DIM, GOLD, TEXT};
use crate::platform::{Req, Slot};

/// REPORT A BUG pressed in a menu: the panel opens on the next frame.
static ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Opens the panel from a menu's button.
pub fn ask() {
    ASKED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// A test's report (`{t:"report",title,text}`): the panel opened, filled and sent.
static FILLED: Mutex<Option<(String, String)>> = Mutex::new(None);

pub fn fill_and_send(title: String, text: String) {
    if let Ok(mut f) = FILLED.lock() {
        *f = Some((title, text));
    }
    ask();
}

/// A picture's longest side, sent.
const SHOT_SIDE: u32 = 1280;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Field {
    #[default]
    Title,
    Text,
}

/// The panel.
#[derive(Resource, Default)]
pub struct ReportPanel {
    pub open: bool,
    kind: usize,
    field: Field,
    title: String,
    text: String,
    with_shot: bool,
    /// The frame shot as the panel opened, as PNG, once it's in.
    shot: Arc<Mutex<Option<Vec<u8>>>>,
    /// Shot asked for; the panel opens when it's in (or a second on).
    opening: Option<f32>,
    sending: Option<Slot>,
    /// What came of the last send.
    pub line: String,
    /// A crash note from the last run: its file and contents.
    crash: Option<(std::path::PathBuf, String)>,
    drawn: String,
}

impl ReportPanel {
    fn key(&self) -> String {
        format!(
            "{}|{}|{:?}|{}|{}|{}|{}|{}|{}",
            self.open,
            self.kind,
            self.field,
            self.title,
            self.text,
            self.with_shot,
            self.sending.is_some(),
            self.line,
            self.crash.is_some()
        )
    }

    fn kind(&self) -> Kind {
        if self.crash.is_some() { Kind::Crash } else { Kind::OFFERED[self.kind % Kind::OFFERED.len()] }
    }

    /// Opens it: the frame shot first.
    pub fn ask(&mut self, commands: &mut Commands, now: f32) {
        if self.open || self.opening.is_some() {
            return;
        }
        let slot = self.shot.clone();
        if let Ok(mut s) = slot.lock() {
            *s = None;
        }
        commands.spawn(Screenshot::primary_window()).observe(move |cap: On<ScreenshotCaptured>| {
            if let Ok(mut s) = slot.lock() {
                *s = png(cap.image.clone());
            }
        });
        self.opening = Some(now);
    }
}

/// The frame as a PNG, its longest side at most [`SHOT_SIDE`].
fn png(img: Image) -> Option<Vec<u8>> {
    let mut rgb = img.try_into_dynamic().ok()?.to_rgb8();
    let (w, h) = rgb.dimensions();
    if w.max(h) > SHOT_SIDE {
        let k = SHOT_SIDE as f32 / w.max(h) as f32;
        rgb = image::imageops::resize(
            &rgb,
            (w as f32 * k) as u32,
            (h as f32 * k) as u32,
            image::imageops::FilterType::Triangle,
        );
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(rgb).write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[derive(Component)]
struct PanelRoot;

/// What a click on the panel does.
#[derive(Component, Clone, Copy, Debug)]
enum Do {
    Kind(usize),
    Field(Field),
    Shot,
    Send,
    Close,
    /// The crash note: put away without sending.
    Dismiss,
}

pub struct ReportPlugin;

impl Plugin for ReportPlugin {
    fn build(&self, app: &mut App) {
        let mut panel = ReportPanel { with_shot: true, ..default() };
        panel.crash = crate::platform::last_crash();
        if panel.crash.is_some() {
            panel.open = true;
            panel.title = "Eloram closed unexpectedly last time".into();
        }
        app.insert_resource(panel)
            .add_systems(PreUpdate, typing.after(bevy::input::InputSystems))
            .add_systems(Update, (press, answer, draw, publish).chain());
    }
}

/// F8 opens it; while it's open, what's typed goes into it and nowhere else.
fn typing(
    mut commands: Commands,
    mut panel: ResMut<ReportPanel>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut chars: MessageReader<KeyboardInput>,
    time: Res<Time<Real>>,
) {
    let now = time.elapsed_secs();
    if let Some(at) = panel.opening {
        let shot_in = panel.shot.lock().is_ok_and(|s| s.is_some());
        if shot_in || now - at > 1.5 {
            panel.opening = None;
            panel.open = true;
            if let Some((title, text)) = FILLED.lock().ok().and_then(|mut f| f.take()) {
                panel.title = title;
                panel.text = text;
                send(&mut panel);
            }
        }
    }
    if !panel.open {
        chars.clear();
        if keys.just_pressed(KeyCode::F8) || ASKED.swap(false, std::sync::atomic::Ordering::Relaxed) {
            panel.ask(&mut commands, now);
            keys.clear_just_pressed(KeyCode::F8);
        }
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    for ev in chars.read() {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Escape => close(&mut panel),
            Key::Tab => panel.field = if panel.field == Field::Title { Field::Text } else { Field::Title },
            Key::Enter if ctrl => send(&mut panel),
            Key::Enter if panel.field == Field::Title => panel.field = Field::Text,
            Key::Enter => {
                if panel.text.chars().count() < bugs::TEXT_MAX {
                    panel.text.push('\n');
                }
            }
            Key::Backspace => {
                let f = panel.field;
                match f {
                    Field::Title => panel.title.pop(),
                    Field::Text => panel.text.pop(),
                };
            }
            Key::Space => push(&mut panel, " "),
            Key::Character(s) if !ctrl => push(&mut panel, s.as_str()),
            _ => {}
        }
    }
    // Nothing typed here reaches the game.
    keys.reset_all();
}

fn push(panel: &mut ReportPanel, s: &str) {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    match panel.field {
        Field::Title if panel.title.chars().count() < bugs::TITLE_MAX => panel.title.push_str(&s),
        Field::Text if panel.text.chars().count() < bugs::TEXT_MAX => panel.text.push_str(&s),
        _ => {}
    }
}

fn close(panel: &mut ReportPanel) {
    panel.open = false;
    panel.title.clear();
    panel.text.clear();
    panel.field = Field::Title;
}

fn send(panel: &mut ReportPanel) {
    if panel.sending.is_some() {
        return;
    }
    let report = Report {
        kind: panel.kind(),
        title: panel.title.clone(),
        text: panel.text.clone(),
        build: bugs::Build {
            name: crate::build_name().to_string(),
            platform: platform(),
            runtime: if crate::platform::DESKTOP { "desktop" } else { "browser" },
        },
        at: crate::platform::now_s() as u64,
        crash_at: panel.crash.as_ref().map(|(_, c)| crash_place(c)).unwrap_or_default(),
        phase: String::new(),
    };
    let shot = if panel.with_shot && panel.crash.is_none() {
        panel.shot.lock().ok().and_then(|s| s.clone())
    } else {
        None
    };
    let backtrace = panel.crash.as_ref().map(|(_, c)| c.as_bytes().to_vec());
    let body = report.submission(shot.as_deref(), backtrace.as_deref());
    // Plain text: the intake reads the body as it comes, and a page on another origin sends it
    // without asking first.
    let req = Req::post(intake(), body).cap(16 * 1024).text();
    panel.sending = Some(crate::platform::fetch(req));
    panel.line = "Sending...".into();
}

/// The intake (`EL_BUGS_URL` on the desktop for tests, never the real one from a test).
fn intake() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(u) = std::env::var("EL_BUGS_URL")
        && !u.is_empty()
    {
        return u;
    }
    bugs::url()
}

fn platform() -> &'static str {
    if !crate::platform::DESKTOP {
        "web"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// A crash note's place: its `panicked at` line.
fn crash_place(note: &str) -> String {
    note.lines().find(|l| l.contains("panicked at")).unwrap_or("").chars().take(200).collect()
}

fn answer(mut panel: ResMut<ReportPanel>) {
    let Some(a) = panel.sending.as_ref().and_then(Slot::take) else { return };
    panel.sending = None;
    match a.and_then(|b| bugs::receipt(&b)) {
        Ok(id) => {
            panel.line = format!("Sent, thank you: report {id}.");
            if let Some((path, _)) = panel.crash.take() {
                crate::platform::put_away_crash(&path);
            }
            close(&mut panel);
        }
        Err(why) => panel.line = format!("Not sent: {why}"),
    }
}

fn press(q: Query<(&Interaction, &Do), Changed<Interaction>>, mut panel: ResMut<ReportPanel>) {
    for (i, d) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        match *d {
            Do::Kind(k) => panel.kind = k,
            Do::Field(f) => panel.field = f,
            Do::Shot => panel.with_shot = !panel.with_shot,
            Do::Send => send(&mut panel),
            Do::Close => close(&mut panel),
            Do::Dismiss => {
                if let Some((path, _)) = panel.crash.take() {
                    crate::platform::put_away_crash(&path);
                }
                close(&mut panel);
            }
        }
    }
}

fn draw(mut commands: Commands, mut panel: ResMut<ReportPanel>, roots: Query<Entity, With<PanelRoot>>) {
    let k = panel.key();
    if k == panel.drawn {
        return;
    }
    panel.drawn = k;
    for r in &roots {
        commands.entity(r).despawn();
    }
    if !panel.open {
        return;
    }
    let root = commands
        .spawn((
            PanelRoot,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            GlobalZIndex(60),
        ))
        .id();
    let card = commands
        .spawn((
            Node {
                width: px(640),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(10),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(ui::INK),
            BorderColor::all(RULE),
        ))
        .id();
    commands.entity(root).add_child(card);
    let c = &mut commands;
    let crash = panel.crash.is_some();
    let head =
        c.spawn(strong(if crash { "ELORAM CLOSED UNEXPECTEDLY" } else { "REPORT A BUG" }, 28.0, TITLE)).id();
    let blurb = if crash {
        "Last time, Eloram stopped. Sending the note it left (what broke, where, this build) helps us fix it. It says nothing about you."
    } else {
        "What went wrong, in a line, and more if you like. It goes to Eloram's makers on Elo Pros with this build's name and, if you leave it on, the picture of the screen as you pressed F8."
    };
    let b = c.spawn(label(blurb, 15.0, DIM)).id();
    c.entity(card).add_children(&[head, b]);
    if !crash {
        let kinds = c
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(6),
                row_gap: px(6),
                ..default()
            })
            .id();
        for (i, k) in Kind::OFFERED.iter().enumerate() {
            let on = panel.kind % Kind::OFFERED.len() == i;
            let bt = c.spawn((ui::button(k.label(), 0.0, on), Do::Kind(i))).id();
            c.entity(kinds).add_child(bt);
        }
        c.entity(card).add_child(kinds);
    }
    let cursor = |on: bool| if on { "_" } else { "" };
    let title_on = panel.field == Field::Title;
    let t = c
        .spawn((
            Button,
            Do::Field(Field::Title),
            Node { padding: UiRect::all(px(8)), border: UiRect::all(px(1)), ..default() },
            BorderColor::all(if title_on { GOLD } else { RULE }),
            children![label(
                if panel.title.is_empty() && !title_on {
                    "A title (Tab to move)".to_string()
                } else {
                    format!("{}{}", panel.title, cursor(title_on))
                },
                16.0,
                if panel.title.is_empty() { FAINT } else { TEXT },
            )],
        ))
        .id();
    let text_on = panel.field == Field::Text;
    let x = c
        .spawn((
            Button,
            Do::Field(Field::Text),
            Node {
                padding: UiRect::all(px(8)),
                min_height: px(120),
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(if text_on { GOLD } else { RULE }),
            children![label(
                if panel.text.is_empty() && !text_on {
                    "What happened, and what you expected".to_string()
                } else {
                    format!("{}{}", panel.text, cursor(text_on))
                },
                15.0,
                if panel.text.is_empty() { FAINT } else { TEXT },
            )],
        ))
        .id();
    c.entity(card).add_children(&[t, x]);
    let row = c
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(8),
            align_items: AlignItems::Center,
            ..default()
        })
        .id();
    if !crash {
        let shot = c
            .spawn((
                ui::button(if panel.with_shot { "PICTURE: ON" } else { "PICTURE: OFF" }, 0.0, false),
                Do::Shot,
            ))
            .id();
        c.entity(row).add_child(shot);
    }
    let gap = c.spawn(Node { flex_grow: 1.0, ..default() }).id();
    let close = c
        .spawn((
            ui::button(if crash { "DON'T SEND" } else { "CLOSE" }, 0.0, false),
            if crash { Do::Dismiss } else { Do::Close },
        ))
        .id();
    let send = c
        .spawn((ui::button(if panel.sending.is_some() { "SENDING..." } else { "SEND" }, 0.0, true), Do::Send))
        .id();
    c.entity(row).add_children(&[gap, close, send]);
    c.entity(card).add_child(row);
    let hint = c.spawn(label("Ctrl+Enter sends, Esc closes", 13.0, FAINT)).id();
    c.entity(card).add_child(hint);
    if !panel.line.is_empty() {
        let bad = panel.line.starts_with("Not sent");
        let l = c.spawn(label(panel.line.clone(), 15.0, if bad { BAD } else { GOLD })).id();
        c.entity(card).add_child(l);
    }
}

fn publish(panel: Res<ReportPanel>, mut dev: ResMut<crate::dev_hooks::DevStatus>) {
    dev.set("report_open", panel.open);
    dev.set("report_line", panel.line.clone());
}

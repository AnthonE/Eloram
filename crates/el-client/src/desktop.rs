//! The desktop's start (`src/bin/eloram.rs`), in Gates' order (`crates/client/src/bin/gates.rs`,
//! `AnthonE/Gates`, MIT, MoreRight DAO): the crash note's hook first, then the arguments, the
//! keyboard library X11 needs, the launcher, and the window.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::config::native::{Parsed, USAGE, parse};

/// Where a crash's note goes: the launcher's folder for it (`ELO_CRASH_DIR`, which it offers to
/// send after an abnormal exit), else the config directory's `crashes/`.
fn crash_dir() -> Option<PathBuf> {
    std::env::var_os("ELO_CRASH_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::platform::config_dir().map(|d| d.join("crashes")))
}

/// The build's name: the version and the commit it was built from (`build.rs`).
pub const BUILD: &str = env!("EL_BUILD");

/// Writes a note of the panic (what, where, the backtrace) for the launcher to offer to send.
fn install_crash_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = crash_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let at =
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let mut note = format!(
                "Eloram {BUILD} crashed\nos: {} {}\n{info}\n\nbacktrace:\n{}\n",
                std::env::consts::OS,
                std::env::consts::ARCH,
                std::backtrace::Backtrace::force_capture()
            );
            let text = info.to_string();
            if text.contains("GPU") || text.contains("adapter") {
                note.push_str(
                    "\nNo graphics adapter: Eloram draws with Vulkan (Linux: mesa-vulkan-drivers, or your GPU's driver) or DirectX 12.\n",
                );
            }
            note.truncate(256 * 1024);
            if let Ok(mut f) = std::fs::File::create(dir.join(format!("eloram-{at}.txt"))) {
                let _ = f.write_all(note.as_bytes());
            }
        }
        previous(info);
    }));
}

/// Without Wayland, X11's keyboard needs `libxkbcommon-x11.so.0`, which winit loads at run time:
/// without it the window dies with a message nobody would understand. Looked for by name in the
/// loader's cache and the usual places (no `dlopen`: nothing unsafe), before any window.
#[cfg(target_os = "linux")]
fn x11_keyboard_missing() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    const LIB: &str = "libxkbcommon-x11.so.0";
    let mut dirs: Vec<PathBuf> = std::env::var("LD_LIBRARY_PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect();
    for d in [
        "/usr/lib/x86_64-linux-gnu",
        "/lib/x86_64-linux-gnu",
        "/usr/lib64",
        "/usr/lib",
        "/lib64",
        "/usr/local/lib",
    ] {
        dirs.push(PathBuf::from(d));
    }
    if dirs.iter().any(|d| d.join(LIB).exists()) {
        return false;
    }
    let cached = std::fs::read("/etc/ld.so.cache").unwrap_or_default();
    !cached.windows(LIB.len()).any(|w| w == LIB.as_bytes())
}

#[cfg(not(target_os = "linux"))]
fn x11_keyboard_missing() -> bool {
    false
}

/// The program.
pub fn main() -> ExitCode {
    install_crash_hook();
    crate::platform::start_clock();
    let mut cfg = match parse(std::env::args().skip(1)) {
        Parsed::Run(c) => *c,
        Parsed::Help => {
            println!("{USAGE}");
            // A windowed build on Windows may have no console to print to: the same words in a file
            // beside it, so a check of the build has something to read.
            if cfg!(windows) {
                let _ = std::fs::write("eloram-help.txt", USAGE);
            }
            return ExitCode::SUCCESS;
        }
        Parsed::Bad(why) => {
            eprintln!("eloram: {why}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if x11_keyboard_missing() {
        let line = "Eloram needs libxkbcommon-x11 for the keyboard under X11: \
                    sudo apt install libxkbcommon-x11-0 (Debian, Ubuntu), \
                    sudo dnf install libxkbcommon-x11 (Fedora), \
                    sudo pacman -S libxkbcommon-x11 (Arch).";
        eprintln!("{line}");
        if let Some(dir) = crash_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("eloram-missing-library.txt"), line);
        }
        return ExitCode::from(1);
    }
    // The launcher: who is playing, and the server list it knows. None is a normal state.
    if !cfg.no_launcher {
        let found = crate::launcher::connect();
        if found.servers_url.is_some() && cfg.servers_url.is_none() {
            cfg.servers_url = found.servers_url.clone();
        }
        cfg.launcher = Some(found);
    }
    // A sign-in needs the launcher's wallet (or a resume token it left last time).
    let has_wallet = cfg.launcher.as_ref().is_some_and(|l| l.address.is_some());
    if cfg.sign_in && !has_wallet && cfg.resume.is_none() {
        cfg.sign_in = false;
    }
    crate::app::run(cfg);
    ExitCode::SUCCESS
}

/// The desktop's own systems: the restart a setting asks for, F11, the window's icon, the cursor
/// kept in the window for the edge's panning while playing, and the test hook's screenshot.
pub struct DesktopPlugin;

impl bevy::prelude::Plugin for DesktopPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        use bevy::prelude::*;
        app.add_systems(Update, (restart, fullscreen, icon, confine, screenshot));
    }
}

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, MonitorSelection, PrimaryWindow, WindowMode};

/// `platform::reload` started the next program: this one goes.
fn restart(mut exit: MessageWriter<AppExit>) {
    if crate::platform::native::RESTARTING.load(std::sync::atomic::Ordering::Relaxed) {
        exit.write(AppExit::Success);
    }
}

/// F11: fullscreen (borderless, on the screen it's on) or a window, kept for next time.
fn fullscreen(keys: Res<ButtonInput<KeyCode>>, mut window: Single<&mut Window, With<PrimaryWindow>>) {
    if !keys.just_pressed(KeyCode::F11) {
        return;
    }
    let full = matches!(window.mode, WindowMode::Windowed);
    window.mode =
        if full { WindowMode::BorderlessFullscreen(MonitorSelection::Current) } else { WindowMode::Windowed };
    crate::platform::kv_set("el.fullscreen", if full { "1" } else { "0" });
}

/// The window's icon (`icon/eloram-256.png`, `art/tools/icon.py`), set through winit once the
/// window exists.
fn icon(mut done: Local<bool>, window: Single<Entity, With<PrimaryWindow>>) {
    if *done {
        return;
    }
    let Ok(img) = image::load_from_memory(include_bytes!("../icon/eloram-256.png")) else {
        *done = true;
        return;
    };
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    let Ok(icon) = winit::window::Icon::from_rgba(rgba.into_raw(), w, h) else {
        *done = true;
        return;
    };
    bevy::winit::WINIT_WINDOWS.with_borrow(|ww| {
        if let Some(win) = ww.get_window(*window) {
            win.set_window_icon(Some(icon));
            *done = true;
        }
    });
}

/// While a match is played in a focused window with no menu up, the cursor stays inside it, so
/// pushing it against an edge pans the camera (`camera.rs`), as a MOBA's does.
fn confine(
    session: Res<crate::session::Session>,
    menu: Res<crate::menu::Menu>,
    mut q: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let (window, cursor) = &mut *q;
    let want = if session.phase == crate::session::Phase::Playing && window.focused && !menu.open {
        CursorGrabMode::Confined
    } else {
        CursorGrabMode::None
    };
    if cursor.grab_mode != want {
        cursor.grab_mode = want;
    }
}

/// `--screenshot <png> --screenshot-at <phase|menu>`: once the moment comes and the picture has
/// settled (two seconds on), the window is saved and the program ends.
fn screenshot(
    mut commands: Commands,
    cfg: Res<crate::net::LaunchConfigRes>,
    session: Res<crate::session::Session>,
    time: Res<Time<Real>>,
    mut since: Local<Option<f32>>,
    mut taken: Local<Option<f32>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = cfg.0.screenshot.clone() else { return };
    let now = time.elapsed_secs();
    if let Some(at) = *taken {
        if now - at > 3.0 {
            exit.write(AppExit::Success);
        }
        return;
    }
    let want = cfg.0.screenshot_at.as_deref().unwrap_or("playing");
    if session.phase.name() != want {
        *since = None;
        return;
    }
    let first = *since.get_or_insert(now);
    if now - first < 2.0 {
        return;
    }
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
        .observe(bevy::render::view::screenshot::save_to_disk(path));
    *taken = Some(now);
}

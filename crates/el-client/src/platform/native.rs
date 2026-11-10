//! The desktop's half of [`super`]: a monotonic clock, the settings in a file and the tokens in
//! files of their own under the player's config directory, HTTP on a thread, a process-wide async
//! runtime for the network, and starting over by starting the program again.
//!
//! The directory is `%APPDATA%\eloram`, `~/Library/Application Support/eloram`, or
//! `$XDG_CONFIG_HOME/eloram` (else `~/.config/eloram`): never beside the binary, which the
//! launcher replaces on an update. `settings.toml` is flat `key = "value"` lines (Gates' shape,
//! `crates/client/src/config.rs`): written whole through a temporary file and a rename, keys it
//! doesn't know kept as they were. The tokens (`el.resume`, `el.rejoin`) are files in `secret/`,
//! readable by the player alone, never in the settings and never logged.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use super::{Opened, Req, SECRET_KEYS, Slot};

/// State a network task shares with the frame.
pub struct Shared<T>(Arc<Mutex<T>>);

impl<T> Shared<T> {
    pub fn new(v: T) -> Self {
        Self(Arc::new(Mutex::new(v)))
    }
    pub fn borrow(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
    pub fn borrow_mut(&self) -> MutexGuard<'_, T> {
        self.borrow()
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Default> Default for Shared<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

pub const DESKTOP: bool = true;

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Starts the clock (at the program's start, so its zero is near the window's first frame).
pub fn start_clock() {
    let _ = epoch();
}

pub fn now_ms() -> f64 {
    epoch().elapsed().as_secs_f64() * 1_000.0
}

/// The runtime the network's tasks run on: two threads, for the whole program.
pub fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("el-net")
            .enable_all()
            .build()
            .expect("the network's runtime")
    })
}

/// Runs a future on the network's runtime.
pub fn spawn(f: impl std::future::Future<Output = ()> + Send + 'static) {
    rt().spawn(f);
}

/// Where a name's environment variable points, if it is set and not empty.
fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The player's config directory for Eloram, by the OS's convention (`None`: nowhere to keep
/// anything, and nothing is kept).
pub fn config_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        if let Some(d) = env_dir("EL_CONFIG_DIR") {
            return Some(d);
        }
        let base = if cfg!(windows) {
            env_dir("APPDATA")
        } else if cfg!(target_os = "macos") {
            env_dir("HOME").map(|h| h.join("Library/Application Support"))
        } else {
            env_dir("XDG_CONFIG_HOME").or_else(|| env_dir("HOME").map(|h| h.join(".config")))
        };
        base.map(|b| b.join("eloram"))
    })
    .clone()
}

/// The settings, read once.
fn settings() -> &'static Mutex<BTreeMap<String, String>> {
    static S: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    S.get_or_init(|| {
        let text = config_dir().and_then(|d| std::fs::read_to_string(d.join("settings.toml")).ok());
        Mutex::new(text.map(|t| parse_settings(&t)).unwrap_or_default())
    })
}

/// `key = "value"` lines (`#` starts a comment); a line that doesn't read is skipped.
pub fn parse_settings(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        let Some(v) = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else { continue };
        let mut value = String::new();
        let mut chars = v.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => value.push('\n'),
                    Some(o) => value.push(o),
                    None => {}
                }
            } else {
                value.push(c);
            }
        }
        if !k.is_empty() {
            out.insert(k.to_string(), value);
        }
    }
    out
}

pub fn write_settings(map: &BTreeMap<String, String>) -> String {
    let mut out =
        String::from("# Eloram's settings: the game writes this file; keys it doesn't know are kept.\n");
    for (k, v) in map {
        let v = v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        out.push_str(&format!("{k} = \"{v}\"\n"));
    }
    out
}

/// Writes `bytes` to `path` whole: a temporary file beside it, then a rename.
fn write_whole(path: &Path, bytes: &[u8], secret: bool) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if secret {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let _ = secret;
        use std::io::Write;
        o.open(&tmp)?.write_all(bytes)?;
    }
    std::fs::rename(tmp, path)
}

fn secret_path(key: &str) -> Option<PathBuf> {
    config_dir().map(|d| d.join("secret").join(key.trim_start_matches("el.")))
}

pub fn kv_get(key: &str) -> Option<String> {
    if SECRET_KEYS.contains(&key) {
        return secret_path(key).and_then(|p| std::fs::read_to_string(p).ok()).filter(|t| !t.is_empty());
    }
    settings().lock().ok()?.get(key).cloned()
}

pub fn kv_set(key: &str, value: &str) {
    if SECRET_KEYS.contains(&key) {
        if let Some(p) = secret_path(key) {
            let _ = write_whole(&p, value.as_bytes(), true);
        }
        return;
    }
    let Ok(mut s) = settings().lock() else { return };
    if s.get(key).map(String::as_str) == Some(value) {
        return;
    }
    s.insert(key.to_string(), value.to_string());
    if let Some(d) = config_dir() {
        let _ = write_whole(&d.join("settings.toml"), write_settings(&s).as_bytes(), false);
    }
}

pub fn kv_remove(key: &str) {
    if SECRET_KEYS.contains(&key) {
        if let Some(p) = secret_path(key) {
            let _ = std::fs::remove_file(p);
        }
        return;
    }
    let Ok(mut s) = settings().lock() else { return };
    if s.remove(key).is_some()
        && let Some(d) = config_dir()
    {
        let _ = write_whole(&d.join("settings.toml"), write_settings(&s).as_bytes(), false);
    }
}

/// The browser keeps these for a tab; the desktop keeps them in a file, so a restart after a
/// crash takes the seat back.
pub fn tab_get(key: &str) -> Option<String> {
    kv_get(key)
}

pub fn tab_set(key: &str, value: Option<&str>) {
    match value {
        Some(v) => kv_set(key, v),
        None => kv_remove(key),
    }
}

/// The address bar's parameters are the browser's: nothing to take off here.
pub fn forget_param(_param: &str) {}

/// Starts the program again with the arguments it was given, and asks this one to stop (the
/// caller writes `AppExit` once this returns true).
pub fn reload() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn();
    }
    RESTARTING.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Starts again with `--<param>` added (`reload_with("signin", "1")`: a sign-in through the launcher).
pub fn reload_with(param: &str, _value: &str) {
    if let Ok(exe) = std::env::current_exe() {
        let flag = format!("--{param}");
        let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).filter(|a| *a != *flag).collect();
        args.push(flag.into());
        let _ = std::process::Command::new(exe).args(args).spawn();
    }
    RESTARTING.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Set once [`reload`] has started the next one: the app exits at the end of the frame.
pub static RESTARTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Opens `url` through the Elo launcher (`crate::launcher`); without one, nothing opens.
pub fn open_url(url: &str) -> Opened {
    crate::launcher::open(url)
}

fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .user_agent(concat!("eloram/", env!("CARGO_PKG_VERSION")))
            .build();
        ureq::Agent::new_with_config(config)
    })
}

/// HTTP on a thread of its own: the answer lands in the slot.
pub fn fetch(req: Req) -> Slot {
    let slot = Slot::default();
    let out = slot.clone();
    std::thread::Builder::new()
        .name("el-fetch".into())
        .spawn(move || out.put(fetch_blocking(&req)))
        .map_err(|e| slot.put(Err(e.to_string())))
        .ok();
    slot
}

/// The same, waited for (on a thread that may wait).
pub fn fetch_blocking(req: &Req) -> super::Answer {
    let resp = match &req.post {
        Some(body) => agent()
            .post(&req.url)
            .header("content-type", if req.plain { "text/plain" } else { "application/json" })
            .send(&body[..]),
        None => agent().get(&req.url).call(),
    };
    let mut resp = resp.map_err(|e| format!("{}: {e}", req.url))?;
    let status = resp.status().as_u16();
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(req.cap as u64)
        .read_to_vec()
        .map_err(|e| format!("{}: {e}", req.url))?;
    if !(200..300).contains(&status) {
        return Err(answer_error(status, &bytes));
    }
    Ok(bytes)
}

/// A refusal in words: the body's `why`/`detail`/`error` when it says one.
pub(crate) fn answer_error(status: u16, body: &[u8]) -> String {
    let said = serde_json::from_slice::<serde_json::Value>(body).ok().and_then(|v| {
        ["why", "detail", "error"].iter().find_map(|k| v.get(*k).and_then(|x| x.as_str()).map(str::to_owned))
    });
    match said {
        Some(w) => format!("{status}: {w}"),
        None => format!("HTTP {status}"),
    }
}

/// Where a crash's note goes (`desktop.rs`'s panic hook): `$ELO_CRASH_DIR`, else `crashes/` in
/// the settings' folder.
pub fn crash_dir() -> Option<PathBuf> {
    env_dir("ELO_CRASH_DIR").or_else(|| config_dir().map(|d| d.join("crashes")))
}

/// The newest crash note the last run left and nobody has sent or put away, with its words.
pub fn last_crash() -> Option<(PathBuf, String)> {
    let dir = crash_dir()?;
    let mut notes: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("eloram-") && n.ends_with(".txt"))
                && !p.file_name().is_some_and(|n| n == "eloram-missing-library.txt")
        })
        .collect();
    notes.sort();
    let newest = notes.pop()?;
    let words = std::fs::read_to_string(&newest).ok()?;
    Some((newest, words))
}

/// A crash note sent or put away: into `crashes/sent/`, never offered again.
pub fn put_away_crash(path: &std::path::Path) {
    let Some(dir) = path.parent().map(|d| d.join("sent")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let Some(name) = path.file_name() {
        let _ = std::fs::rename(path, dir.join(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_back_as_written_and_odd_lines_are_skipped() {
        let mut m = BTreeMap::new();
        m.insert("el.keys".to_string(), "wasd".to_string());
        m.insert("el.audio".to_string(), r#"{"master":0.5,"x":"a\"b"}"#.to_string());
        m.insert("future.key".to_string(), "kept".to_string());
        let text = write_settings(&m);
        assert_eq!(parse_settings(&text), m);
        let odd = "garbage\nno = quotes\n# a comment\nel.camera = \"locked\"\n";
        let got = parse_settings(odd);
        assert_eq!(got.len(), 1);
        assert_eq!(got["el.camera"], "locked");
    }
}
